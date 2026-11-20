//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, hash::Hash, time::Duration};

use log::*;
use tari_consensus_types::{HighPc, LeafBlock, ProposalCertificate, SignedMessage, Vote};
use tari_ootle_common_types::{Epoch, NodeHeight, committee::Committee, optional::Optional};
use tari_ootle_storage::{
    StateStore,
    StateStoreWriteTransaction,
    consensus_models::{Block, BookkeepingModel},
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tokio::time::Instant;

use super::vote_collector::{ProposalVoteCollector, TimeoutVoteCollector};
use crate::{
    hotstuff::{
        HotstuffConfig,
        LeaderSkipSet,
        ProposalValidationError,
        epoch_state::EpochState,
        error::HotStuffError,
        pacemaker::PaceMaker,
        pacemaker_action::PacemakerActions,
    },
    messages::NewViewMessage,
    tracing::TraceTimer,
    traits::{CertificateStore, ConsensusSpec},
    validations::check_quorum_certificate_signatures,
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_receive_new_view";

/// Maximum burst of NEWVIEWs one committee member may push into leader-skip lookup and certificate verification
/// ahead of the pacemaker's leader-timeout replenishment rate.
const MAX_NEWVIEW_BURST_PER_SENDER: u32 = 20;

pub struct OnReceiveNewViewHandler<TConsensusSpec: ConsensusSpec> {
    config: HotstuffConfig,
    local_validator_addr: TConsensusSpec::Addr,
    store: TConsensusSpec::StateStore,
    leader_strategy: TConsensusSpec::LeaderStrategy,
    proposal_vote_collector: ProposalVoteCollector<TConsensusSpec>,
    timeout_vote_collector: TimeoutVoteCollector<TConsensusSpec>,
    rate_limiter: NewViewRateLimiter<TConsensusSpec::Addr>,
}

impl<TConsensusSpec> OnReceiveNewViewHandler<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        config: HotstuffConfig,
        local_validator_addr: TConsensusSpec::Addr,
        store: TConsensusSpec::StateStore,
        leader_strategy: TConsensusSpec::LeaderStrategy,
        proposal_vote_collector: ProposalVoteCollector<TConsensusSpec>,
        timeout_vote_collector: TimeoutVoteCollector<TConsensusSpec>,
    ) -> Self {
        let rate_limiter = NewViewRateLimiter::new(PaceMaker::min_leader_timeout_interval(
            config.consensus_constants.pacemaker_block_time,
        ));
        Self {
            config,
            local_validator_addr,
            store,
            leader_strategy,
            proposal_vote_collector,
            timeout_vote_collector,
            rate_limiter,
        }
    }

    #[allow(clippy::too_many_lines)]
    pub async fn handle(
        &mut self,
        pacemaker_actions: &mut PacemakerActions,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        current_height: NodeHeight,
        from: TConsensusSpec::Addr,
        message: NewViewMessage,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "OnReceiveNewView");

        if !message.timeout_claim_matches_high_pc() {
            warn!(
                target: LOG_TARGET,
                "❌ NEWVIEW from {from} claims high certificate height {} but carries {}",
                message.timeout.high_pc_height,
                message.high_pc
            );
            return Ok(());
        }

        let NewViewMessage {
            high_pc,
            last_vote,
            timeout,
        } = message;
        let timeout_height = timeout.height;
        info!(
            target: LOG_TARGET,
            "🌟 NEWVIEW from {from} with timeout height {timeout_height} with qc {high_pc}",
        );
        if high_pc.epoch() != epoch_state.epoch() || timeout.epoch() != epoch_state.epoch() {
            warn!(
                target: LOG_TARGET,
                "❌ NEWVIEW from {from} with qc epoch {} / timeout epoch {} but current epoch is {}",
                high_pc.epoch(),
                timeout.epoch(),
                epoch_state.epoch()
            );
            return Ok(());
        }

        if timeout_height < current_height {
            warn!(target: LOG_TARGET, "❌ Ignoring NEWVIEW for {timeout_height} less than the current {current_height}.");
            return Ok(());
        }

        // The timeout collector verifies the vote's signature. This binds the signer to the sender.
        if !is_signed_by_sender(epoch_state.local_committee(), &from, timeout.public_key()) {
            warn!(
                target: LOG_TARGET,
                "❌ Ignoring NEWVIEW from {from}: its timeout vote names signer {}, which is not the sender's key",
                timeout.public_key()
            );
            return Ok(());
        }

        if !self.admit_sender(epoch_state, &from, timeout_height) {
            return Ok(());
        }

        // A NEWVIEW is addressed to the effective leader of the view it names. Anyone else is not going to act
        // on it, so establish that before verifying the certificate's 2f+1 signatures, which is the expensive
        // part of handling this message. The sender picked that leader from the liveness state anchored on the
        // certificate it reports, so the same certificate decides it here.
        let skip_set = self.store.with_read_tx(|tx| {
            LeaderSkipSet::load_for_justify(
                tx,
                epoch_state.epoch(),
                high_pc.height(),
                epoch_state.local_committee(),
                &self.config.consensus_constants.liveness_thresholds(),
            )
        })?;
        let (leader, _) =
            skip_set.effective_leader(&self.leader_strategy, epoch_state.local_committee(), timeout_height);

        if *leader != self.local_validator_addr {
            warn!(target: LOG_TARGET, "❌ NEWVIEW failed, leader is {} at {}. Our address is {}", leader, timeout_height, self.local_validator_addr);
            return Ok(());
        }

        if let Err(err) = self.validate_qc(&high_pc, epoch_state, self.proposal_vote_collector.signing_service()) {
            warn!(target: LOG_TARGET, "❌ NEWVIEW: Invalid QC: {}", err);
            return Ok(());
        }

        // A NEWVIEW reports the certificate its sender holds. One ahead of ours has to become ours before we
        // propose: a replica locked above our justify block rejects the proposal, and the timeout certificate this
        // vote goes into binds its leader to a certificate at least that high. One level with or behind ours is
        // worth no write - every sender of a view reports the same certificate in the common case - but its
        // timeout vote still counts towards the quorum that ends the view, so the message carries on either way.
        let is_ahead_of_ours = self.store.with_read_tx(|tx| {
            let local_high_pc = HighPc::get(tx, epoch_state.epoch())?;
            // A report level with ours leaves nothing to catch up to: `update_highest` sets the leaf block, which
            // fails when the block is missing, so the certificate we hold at that height always has its block. A
            // different certificate at the same height is equivocation, and the branch it names is one the lock
            // rule keeps from committing rather than one to sync onto.
            if local_high_pc.block_height >= high_pc.height() {
                return Ok(false);
            }

            if !Block::record_exists(tx, &high_pc.calculate_block_id())? {
                // Sync if we do not have the block for this valid QC
                let local_height = LeafBlock::get(tx, epoch_state.epoch())
                    .optional()?
                    .map(|leaf| leaf.height())
                    .unwrap_or_default();
                return Err(HotStuffError::FallenBehind {
                    local_epoch: epoch_state.epoch(),
                    local_height,
                    qc_epoch: high_pc.epoch(),
                    qc_height: high_pc.height(),
                });
            }

            Ok(true)
        })?;

        if is_ahead_of_ours {
            self.store.with_write_tx(|tx| high_pc.update_highest(tx))?;
        }

        if let Some(vote) = last_vote {
            debug!(
                target: LOG_TARGET,
                "🔥 Receive VOTE with NEWVIEW for node {} {} from {}", vote.height(), vote.block_id, from,
            );
            // HighPc is updated if a quorum is reached in the collector, and will be used if we propose
            if let Err(err) = self
                .proposal_vote_collector
                .check_and_collect_vote(from.clone(), current_height, epoch_state, vote)
                .await
            {
                warn!(target: LOG_TARGET, "❌ Error handling vote: {}", err);
            }
        }

        // Take note of unique NEWVIEWs so that we can count them
        let (timeout_certificate, high_tc) = match self
            .timeout_vote_collector
            .check_and_collect_vote(from, current_height, epoch_state, timeout)
            .await
        {
            Ok(Some(tc)) => tc,
            Ok(None) => {
                debug!(target: LOG_TARGET, "🌟 Received NEWVIEW but quorum is not yet reached.");
                return Ok(());
            },
            Err(err) => {
                warn!(target: LOG_TARGET, "❌ Error handling timeout vote: {}", err);
                return Ok(());
            },
        };

        let threshold = epoch_state.local_committee_info().quorum_threshold();

        info!(target: LOG_TARGET, "🌟✅ NEWVIEW height {} (high_tc: {}) has reached quorum ({}/{})", timeout_height, high_tc, timeout_certificate.num_signatures(), threshold.value());
        if timeout_certificate.calculate_id() == *high_tc.id() {
            info!(target: LOG_TARGET, "🕒️ New HIGH TC {}", timeout_certificate);
            // Clear the last sent new view since we have a new certificate
            self.store.with_write_tx(|tx| tx.last_sent_new_view_clear())?;
            pacemaker_actions.force_beat(high_tc.height());
        } else {
            info!(target: LOG_TARGET, "❓️ New TC from votes {} but it is not the highest TC {}", timeout_certificate, high_tc);
        }

        Ok(())
    }

    fn validate_qc(
        &self,
        qc: &ProposalCertificate,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        vote_signing_service: &TConsensusSpec::SignerService,
    ) -> Result<(), ProposalValidationError> {
        if qc.epoch() != epoch_state.epoch() {
            return Err(ProposalValidationError::InvalidEpochInQc {
                block_id: qc.calculate_block_id(),
                qc_id: qc.calculate_id(),
                qc_epoch: qc.epoch(),
                current_epoch: epoch_state.epoch(),
            });
        }
        let local_shard_group = epoch_state.local_committee_info().shard_group();
        if qc.shard_group() != local_shard_group {
            return Err(ProposalValidationError::InvalidShardGroup {
                block_id: qc.calculate_block_id(),
                shard_group: qc.shard_group(),
                details: format!(
                    "QC shard group {} does not match local shard group {}",
                    qc.shard_group(),
                    local_shard_group
                ),
            });
        }
        check_quorum_certificate_signatures::<TConsensusSpec>(
            self.proposal_vote_collector.network(),
            qc.into(),
            epoch_state.local_committee(),
            vote_signing_service,
        )?;
        Ok(())
    }

    fn admit_sender(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        from: &TConsensusSpec::Addr,
        timeout_height: NodeHeight,
    ) -> bool {
        match self
            .rate_limiter
            .check(epoch_state.epoch(), epoch_state.local_committee(), from, Instant::now())
        {
            RateDecision::Admit => true,
            RateDecision::ThrottleFirst => {
                warn!(
                    target: LOG_TARGET,
                    "❌ Throttling NEWVIEW flood from {from} at height {timeout_height}: exceeded burst of {MAX_NEWVIEW_BURST_PER_SENDER}",
                );
                false
            },
            RateDecision::ThrottleRepeat => {
                debug!(
                    target: LOG_TARGET,
                    "❌ Dropping rate-limited NEWVIEW from {from} at height {timeout_height}",
                );
                false
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RateDecision {
    Admit,
    ThrottleFirst,
    ThrottleRepeat,
}

struct SenderQuota {
    tokens: u32,
    last_refill: Instant,
    throttled_in_burst: bool,
}

impl SenderQuota {
    fn refill(&mut self, now: Instant, interval: Duration) {
        if interval.is_zero() {
            self.tokens = MAX_NEWVIEW_BURST_PER_SENDER;
            self.last_refill = now;
            return;
        }
        let elapsed = now.saturating_duration_since(self.last_refill);
        let steps = elapsed.as_nanos() / interval.as_nanos();
        if steps == 0 {
            return;
        }
        let added = u32::try_from(steps).unwrap_or(MAX_NEWVIEW_BURST_PER_SENDER);
        self.tokens = self.tokens.saturating_add(added).min(MAX_NEWVIEW_BURST_PER_SENDER);
        if self.tokens == MAX_NEWVIEW_BURST_PER_SENDER {
            self.last_refill = now;
        } else {
            self.last_refill += interval.saturating_mul(added);
        }
    }
}

/// Per-sender token bucket bounding how fast any single committee member can drive NEWVIEW verification work.
///
/// An honest validator sends at most one NEWVIEW per [`PaceMaker::min_leader_timeout_interval`], regardless of how
/// far `LastSentNewView` has drifted above `current_height` during quorum loss. Replenishing one token per minimum
/// leader-timeout interval with a [`MAX_NEWVIEW_BURST_PER_SENDER`] allowance admits arbitrary-duration recovery and
/// transient delivery bursts while rejecting unbounded future-height floods before database or signature work.
struct NewViewRateLimiter<TAddr> {
    epoch: Option<Epoch>,
    refill_interval: Duration,
    quotas: HashMap<TAddr, SenderQuota>,
}

impl<TAddr: Eq + Hash + Clone> NewViewRateLimiter<TAddr> {
    fn new(refill_interval: Duration) -> Self {
        Self {
            epoch: None,
            refill_interval,
            quotas: HashMap::new(),
        }
    }

    fn check(&mut self, epoch: Epoch, committee: &Committee<TAddr>, sender: &TAddr, now: Instant) -> RateDecision {
        if self.epoch != Some(epoch) {
            self.quotas.retain(|addr, _| committee.contains(addr));
            self.epoch = Some(epoch);
        }

        let Some(quota) = self.quotas.get_mut(sender) else {
            self.quotas.insert(sender.clone(), SenderQuota {
                tokens: MAX_NEWVIEW_BURST_PER_SENDER - 1,
                last_refill: now,
                throttled_in_burst: false,
            });
            return RateDecision::Admit;
        };

        quota.refill(now, self.refill_interval);
        if quota.tokens > 0 {
            quota.tokens -= 1;
            quota.throttled_in_burst = false;
            return RateDecision::Admit;
        }

        if quota.throttled_in_burst {
            RateDecision::ThrottleRepeat
        } else {
            quota.throttled_in_burst = true;
            RateDecision::ThrottleFirst
        }
    }
}

/// Whether `public_key` is the key of the local committee member at address `from`.
fn is_signed_by_sender<TAddr: PartialEq>(
    committee: &Committee<TAddr>,
    from: &TAddr,
    public_key: &RistrettoPublicKeyBytes,
) -> bool {
    committee
        .index_of_public_key(public_key)
        .and_then(|index| committee.get(index))
        .is_some_and(|member| member.address == *from)
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::{VotePower, committee::CommitteeMember};

    use super::*;

    fn key(byte: u8) -> RistrettoPublicKeyBytes {
        RistrettoPublicKeyBytes::from_bytes(&[byte; 32]).unwrap()
    }

    /// A committee whose member at address `key(n)` has public key `key(n + 10)`.
    fn committee() -> Committee<RistrettoPublicKeyBytes> {
        [1u8, 2, 3]
            .into_iter()
            .map(|n| CommitteeMember {
                address: key(n),
                public_key: key(n + 10),
                vote_power: VotePower::of(1),
            })
            .collect()
    }

    #[test]
    fn a_timeout_signed_with_the_senders_key_is_accepted() {
        assert!(is_signed_by_sender(&committee(), &key(2), &key(12)));
    }

    #[test]
    fn a_timeout_signed_by_another_member_is_rejected() {
        assert!(!is_signed_by_sender(&committee(), &key(2), &key(13)));
    }

    #[test]
    fn a_timeout_signed_by_a_non_member_is_rejected() {
        assert!(!is_signed_by_sender(&committee(), &key(2), &key(14)));
    }

    #[test]
    fn rate_limiter_caps_burst_per_sender_and_refills_at_pacemaker_interval() {
        let interval = PaceMaker::min_leader_timeout_interval(Duration::from_secs(10));
        assert_eq!(interval, Duration::from_secs(12));

        let mut limiter = NewViewRateLimiter::new(interval);
        let comm = committee();
        let spammer = key(1);
        let honest = key(2);
        let t0 = Instant::now();

        for _ in 0..MAX_NEWVIEW_BURST_PER_SENDER {
            assert_eq!(limiter.check(Epoch(1), &comm, &spammer, t0), RateDecision::Admit);
        }
        assert_eq!(
            limiter.check(Epoch(1), &comm, &spammer, t0),
            RateDecision::ThrottleFirst
        );
        assert_eq!(
            limiter.check(Epoch(1), &comm, &spammer, t0),
            RateDecision::ThrottleRepeat
        );

        // Another committee member's burst allowance is unaffected by the spammer.
        assert_eq!(limiter.check(Epoch(1), &comm, &honest, t0), RateDecision::Admit);

        // Just shy of one leader-timeout interval still throttles; reaching the interval replenishes one token.
        assert_eq!(
            limiter.check(Epoch(1), &comm, &spammer, t0 + interval - Duration::from_millis(1)),
            RateDecision::ThrottleRepeat
        );
        assert_eq!(
            limiter.check(Epoch(1), &comm, &spammer, t0 + interval),
            RateDecision::Admit
        );
        assert_eq!(
            limiter.check(Epoch(1), &comm, &spammer, t0 + interval),
            RateDecision::ThrottleFirst
        );
    }

    #[test]
    fn rate_limiter_evicts_departed_members_on_epoch_change_and_retains_continuing_quotas() {
        let interval = Duration::from_secs(12);
        let mut limiter = NewViewRateLimiter::new(interval);
        let epoch1_committee = committee();
        let staying_spammer = key(1);
        let departing_member = key(2);
        let t0 = Instant::now();

        for _ in 0..MAX_NEWVIEW_BURST_PER_SENDER {
            assert_eq!(
                limiter.check(Epoch(1), &epoch1_committee, &staying_spammer, t0),
                RateDecision::Admit
            );
        }
        assert_eq!(
            limiter.check(Epoch(1), &epoch1_committee, &departing_member, t0),
            RateDecision::Admit
        );
        assert_eq!(limiter.quotas.len(), 2);

        let epoch2_committee: Committee<RistrettoPublicKeyBytes> = [1u8, 4]
            .into_iter()
            .map(|n| CommitteeMember {
                address: key(n),
                public_key: key(n + 10),
                vote_power: VotePower::of(1),
            })
            .collect();

        // Continuing member cannot reset a depleted bucket across an epoch transition, while departed member is
        // dropped.
        assert_eq!(
            limiter.check(Epoch(2), &epoch2_committee, &staying_spammer, t0),
            RateDecision::ThrottleFirst
        );
        assert_eq!(limiter.quotas.len(), 1);
        assert!(!limiter.quotas.contains_key(&departing_member));
    }
}
