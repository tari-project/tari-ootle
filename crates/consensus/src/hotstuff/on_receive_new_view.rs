//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::{HighPc, LeafBlock, ProposalCertificate, SignedMessage, Vote};
use tari_ootle_common_types::{NodeHeight, committee::Committee, optional::Optional};
use tari_ootle_storage::{
    StateStore,
    StateStoreWriteTransaction,
    consensus_models::{Block, BookkeepingModel},
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use super::vote_collector::{ProposalVoteCollector, TimeoutVoteCollector, exceeds_vote_lookahead};
use crate::{
    hotstuff::{
        HotstuffConfig,
        LeaderSkipSet,
        ProposalValidationError,
        epoch_state::EpochState,
        error::HotStuffError,
        pacemaker::PaceMaker,
    },
    messages::NewViewMessage,
    tracing::TraceTimer,
    traits::{CertificateStore, ConsensusSpec},
    validations::check_quorum_certificate_signatures,
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_receive_new_view";

pub struct OnReceiveNewViewHandler<TConsensusSpec: ConsensusSpec> {
    config: HotstuffConfig,
    local_validator_addr: TConsensusSpec::Addr,
    store: TConsensusSpec::StateStore,
    leader_strategy: TConsensusSpec::LeaderStrategy,
    proposal_vote_collector: ProposalVoteCollector<TConsensusSpec>,
    timeout_vote_collector: TimeoutVoteCollector<TConsensusSpec>,
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
        Self {
            config,
            local_validator_addr,
            store,
            leader_strategy,
            proposal_vote_collector,
            timeout_vote_collector,
        }
    }

    #[allow(clippy::too_many_lines)]
    pub async fn handle(
        &mut self,
        pacemaker: &mut PaceMaker,
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

        if exceeds_vote_lookahead(current_height, timeout_height) {
            warn!(
                target: LOG_TARGET,
                "🗑️ Discarding NEWVIEW for {timeout_height} from {from}: it is too far ahead of our current view {}/{current_height}",
                epoch_state.epoch()
            );
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
            pacemaker.force_beat(high_tc.height());
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
}
