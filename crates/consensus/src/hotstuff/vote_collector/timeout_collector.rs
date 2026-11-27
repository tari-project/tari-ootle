//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::{HighTc, SignedMessage, TcSignatures, TimeoutCertificate, TimeoutVote, Vote};
use tari_ootle_common_types::{Epoch, NodeHeight, VotePower, committee::Committee};
use tari_ootle_storage::{StateStore, global::models::ValidatorNode};
use tari_ootle_transaction::Network;
use tari_sidechain::SignerBitmap;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use super::collector::{DuplicateVoteDetected, VoteCollector};
use crate::{
    hotstuff::{epoch_state::EpochState, error::HotStuffError, vote_collector::helpers::check_eligibility},
    tracing::TraceTimer,
    traits::{CertificateStore, ConsensusSpec, ValidatorSignatureVerifierService},
    validations::signed_vote::SignedTimeoutVote,
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::timeout_collector";

/// Maximum number of distinct timeout views buffered per committee signer at once.
///
/// While a committee lacks quorum, `current_height` stands still as each leader timeout advances
/// `next_timeout_height`, so a fixed lookahead window relative to `current_height` eventually drops every
/// peer's NEWVIEW and prevents recovery. Bounding by signer instead caps total buffered timeout votes at
/// `committee.len() * MAX_TIMEOUT_VOTES_PER_SENDER` while allowing recovery after arbitrarily long stalls.
pub const MAX_TIMEOUT_VOTES_PER_SENDER: usize = 128;

/// What one timeout vote completed in a [`TimeoutVoteCollector`].
#[derive(Debug, Default)]
pub struct CollectedTimeoutVote {
    /// The certificate for the vote's view, formed from votes whose `high_pc_height` claims were checked against
    /// the certificate they came with. Only this certificate may be proposed on.
    pub certificate: Option<(TimeoutCertificate, HighTc)>,
    pub pacing: PacingTimeoutVotes,
}

/// What the timeout votes counted for pacing show about the committee's view.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PacingTimeoutVotes {
    /// The vote's view, when this vote brought the signers that timed it out to a quorum.
    pub timed_out_view: Option<NodeHeight>,
    /// The highest view that more than the committee's tolerated faulty power has timed out, by each signer's highest
    /// held vote. At least one correct member has named this view or a higher one.
    pub joinable_view: Option<NodeHeight>,
}

/// Counts leader timeout votes in two stores.
///
/// - Certified votes arrive in a NEWVIEW at the leader of the view they name, together with the certificate their
///   `high_pc_height` claims. They form the [`TimeoutCertificate`] that leader proposes on, and a block's justify must
///   reach the highest claim in it.
/// - Every timeout vote, certified or not, is counted for pacing: a quorum of them tells any member that the view timed
///   out. A vote outside a NEWVIEW carries no certificate, so its claim is unchecked and it never enters a certificate
///   that is proposed on.
#[derive(Clone)]
pub struct TimeoutVoteCollector<TConsensusSpec: ConsensusSpec> {
    network: Network,
    certified_votes: VoteCollector<TimeoutVote>,
    pacing_votes: VoteCollector<TimeoutVote>,
    store: TConsensusSpec::StateStore,
    epoch_manager: TConsensusSpec::EpochManager,
    vote_signer_service: TConsensusSpec::SignerService,
}

impl<TConsensusSpec> TimeoutVoteCollector<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        network: Network,
        store: TConsensusSpec::StateStore,
        epoch_manager: TConsensusSpec::EpochManager,
        vote_signer_service: TConsensusSpec::SignerService,
    ) -> Self {
        Self {
            network,
            store,
            certified_votes: VoteCollector::with_per_sender_limit(MAX_TIMEOUT_VOTES_PER_SENDER),
            pacing_votes: VoteCollector::with_per_sender_limit(MAX_TIMEOUT_VOTES_PER_SENDER),
            epoch_manager,
            vote_signer_service,
        }
    }

    /// Collects a vote whose `high_pc_height` claim the caller checked against the certificate it came with.
    pub async fn check_and_collect_vote(
        &self,
        from: TConsensusSpec::Addr,
        current_height: NodeHeight,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        vote: TimeoutVote,
    ) -> Result<CollectedTimeoutVote, HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "check_and_collect_vote (TimeoutVote)");
        let Some(sender_vn) = self.check_vote(from, current_height, epoch_state, &vote).await? else {
            return Ok(CollectedTimeoutVote::default());
        };

        let pacing = self
            .count_pacing_vote(&sender_vn, current_height, epoch_state, vote.clone())
            .await;

        let current_epoch = epoch_state.epoch();
        let height = vote.height;
        let result = self
            .certified_votes
            .collect_vote(
                &sender_vn,
                current_epoch,
                current_height,
                vote,
                epoch_state.local_committee(),
            )
            .await;

        let certificate = match result {
            Ok(Some(quorum_votes)) => {
                let new_tc =
                    create_timeout_certificate(epoch_state.local_committee(), current_epoch, height, &quorum_votes)?;
                let high_tc = self.store.with_write_tx(|tx| new_tc.update_highest(tx))?;
                if new_tc.calculate_id() == *high_tc.id() {
                    info!(target: LOG_TARGET, "🕒️ New HIGH {}", new_tc);
                } else {
                    info!(target: LOG_TARGET, "❓️ New TC from votes {} but it is not the highest TC {}", new_tc, high_tc);
                }
                Some((new_tc, high_tc))
            },
            Ok(None) => {
                debug!(target: LOG_TARGET, "🟡 No quorum reached yet for TimeoutVote at height {}", height);
                None
            },
            Err(duplicate) => {
                log_duplicate(&duplicate);
                None
            },
        };

        Ok(CollectedTimeoutVote { certificate, pacing })
    }

    /// Counts a vote that arrived without the certificate its `high_pc_height` claims.
    pub async fn check_and_count_vote(
        &self,
        from: TConsensusSpec::Addr,
        current_height: NodeHeight,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        vote: TimeoutVote,
    ) -> Result<PacingTimeoutVotes, HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "check_and_count_vote (TimeoutVote)");
        let Some(sender_vn) = self.check_vote(from, current_height, epoch_state, &vote).await? else {
            return Ok(PacingTimeoutVotes::default());
        };
        Ok(self
            .count_pacing_vote(&sender_vn, current_height, epoch_state, vote)
            .await)
    }

    /// The highest view `public_key` has timed out in `epoch`, among the timeout votes held from it.
    pub async fn highest_timed_out_view_of(
        &self,
        epoch: Epoch,
        public_key: &RistrettoPublicKeyBytes,
    ) -> Option<NodeHeight> {
        self.pacing_votes.highest_view_of(epoch, public_key).await
    }

    /// The registered validator that signed `vote`, once the vote is known to be from a local committee member,
    /// for the current epoch and correctly signed. `None` for a vote behind the current view.
    async fn check_vote(
        &self,
        from: TConsensusSpec::Addr,
        current_height: NodeHeight,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        vote: &TimeoutVote,
    ) -> Result<Option<ValidatorNode<TConsensusSpec::Addr>>, HotStuffError> {
        debug!(
            target: LOG_TARGET,
            "📬 Validating timeout vote message from {from}: {vote}"
        );

        let current_epoch = epoch_state.epoch();

        // Check epoch and lower view bound before querying epoch_manager or verifying the signature.
        // Upper buffering is bounded per signer by `MAX_TIMEOUT_VOTES_PER_SENDER` rather than a fixed lookahead
        // window around `current_height`, which stands still while a committee lacks quorum.
        check_timeout_vote_epoch(current_epoch, vote)?;
        if vote.height < current_height {
            debug!(
                target: LOG_TARGET,
                "🗑️ Discarding {} from {}: it is behind our current view {}/{}",
                vote,
                from,
                current_epoch,
                current_height
            );
            return Ok(None);
        }

        let sender_vn =
            check_eligibility::<TConsensusSpec, _>(&self.epoch_manager, from, vote, epoch_state.local_committee_info())
                .await?;
        self.validate_vote(current_epoch, vote)?;
        Ok(Some(sender_vn))
    }

    async fn count_pacing_vote(
        &self,
        sender_vn: &ValidatorNode<TConsensusSpec::Addr>,
        current_height: NodeHeight,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        vote: TimeoutVote,
    ) -> PacingTimeoutVotes {
        let epoch = epoch_state.epoch();
        let committee = epoch_state.local_committee();
        let height = vote.height;
        let timed_out_view = match self
            .pacing_votes
            .collect_vote(sender_vn, epoch, current_height, vote, committee)
            .await
        {
            Ok(Some(_)) => {
                info!(target: LOG_TARGET, "🕒️ A quorum of the committee timed out view {epoch}/{height}");
                Some(height)
            },
            Ok(None) => None,
            Err(duplicate) => {
                log_duplicate(&duplicate);
                None
            },
        };
        let joinable_view = self
            .pacing_votes
            .highest_view_held_by(
                epoch,
                committee,
                committee.max_failures().saturating_add(VotePower::of(1)),
            )
            .await;
        PacingTimeoutVotes {
            timed_out_view,
            joinable_view,
        }
    }

    fn validate_vote(&self, current_epoch: Epoch, vote: &TimeoutVote) -> Result<(), HotStuffError> {
        check_timeout_vote_epoch(current_epoch, vote)?;

        let signed_vote = SignedTimeoutVote {
            message: vote.to_message(self.network),
            signature: &vote.signature,
        };
        if !self.vote_signer_service.verify(&signed_vote) {
            return Err(HotStuffError::InvalidVoteSignature {
                signer_public_key: *vote.public_key(),
            });
        }

        Ok(())
    }
}

/// Not equivocation, and no evidence can be built from it: a timeout vote signs its view and the height of the
/// certificate its signer holds, and the vote store buckets it under that view, so two votes from one signer differ
/// at most in that height or the signature's nonce. Re-signing one view is something an honest node does, for
/// instance after a restart loses the in-memory record of the NEWVIEW it last sent.
fn log_duplicate(duplicate: &DuplicateVoteDetected<TimeoutVote>) {
    warn!(target: LOG_TARGET, "❌ {}", duplicate);
}

fn check_timeout_vote_epoch(current_epoch: Epoch, vote: &TimeoutVote) -> Result<(), HotStuffError> {
    if current_epoch != vote.epoch() {
        return Err(HotStuffError::InvalidVote {
            signer_public_key: vote.signature.public_key,
            details: format!(
                "Our current view is at epoch {} but the vote was for epoch {}",
                current_epoch,
                vote.epoch()
            ),
        });
    }
    Ok(())
}

/// The certificate for `votes`, each a verified timeout vote by a member of `committee`.
fn create_timeout_certificate<TAddr: PartialEq>(
    committee: &Committee<TAddr>,
    epoch: Epoch,
    height: NodeHeight,
    votes: &[TimeoutVote],
) -> Result<TimeoutCertificate, HotStuffError> {
    let mut signers = votes
        .iter()
        .map(|vote| {
            let index = committee.index_of_public_key(vote.public_key()).ok_or_else(|| {
                HotStuffError::InvariantError(format!(
                    "Timeout vote signer {} is not a member of the committee",
                    vote.public_key()
                ))
            })?;
            Ok((index, vote.as_signed_timeout()))
        })
        .collect::<Result<Vec<_>, HotStuffError>>()?;
    signers.sort_by_key(|(index, _)| *index);
    let bitmap = SignerBitmap::from_indices(committee.len(), signers.iter().map(|(index, _)| *index))
        .map_err(|e| HotStuffError::InvariantError(format!("Timeout vote signers do not index the committee: {e}")))?;
    let signatures = TcSignatures::Schnorr {
        signers: bitmap,
        timeouts: signers.into_iter().map(|(_, timeout)| timeout).collect(),
    };
    Ok(TimeoutCertificate::new(epoch, height, signatures))
}

#[cfg(test)]
mod tests {
    use tari_consensus_types::{SignedTimeout, ValidatorSignatureBytes};
    use tari_ootle_common_types::{VotePower, committee::CommitteeMember};
    use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

    use super::*;

    fn public_key(byte: u8) -> RistrettoPublicKeyBytes {
        RistrettoPublicKeyBytes::from_bytes(&[byte; 32]).unwrap()
    }

    fn vote(byte: u8) -> TimeoutVote {
        TimeoutVote {
            epoch: Epoch(1),
            height: NodeHeight(5),
            high_pc_height: NodeHeight(u64::from(byte)),
            signature: ValidatorSignatureBytes::new(public_key(byte), SchnorrSignatureBytes::zero()),
        }
    }

    fn committee(bytes: &[u8]) -> Committee<RistrettoPublicKeyBytes> {
        bytes
            .iter()
            .map(|b| CommitteeMember {
                address: public_key(*b),
                public_key: public_key(*b),
                vote_power: VotePower::of(1),
            })
            .collect()
    }

    #[test]
    fn it_indexes_signers_by_committee_position() {
        let committee = committee(&[4, 2, 3, 1]);
        let orders = [[3, 1, 2], [1, 2, 3], [2, 3, 1]];
        let certificates = orders
            .iter()
            .map(|order| {
                let votes = order.iter().map(|b| vote(*b)).collect::<Vec<_>>();
                create_timeout_certificate(&committee, Epoch(1), NodeHeight(5), &votes).unwrap()
            })
            .collect::<Vec<_>>();

        let expected = TcSignatures::Schnorr {
            signers: SignerBitmap::from_indices(4, [0, 1, 2]).unwrap(),
            timeouts: [1, 2, 3]
                .iter()
                .map(|b| SignedTimeout {
                    high_pc_height: NodeHeight(*b),
                    signature: SchnorrSignatureBytes::zero(),
                })
                .collect(),
        };
        for certificate in &certificates {
            assert_eq!(*certificate.signatures(), expected);
            certificate.signatures().check_well_formed(committee.len()).unwrap();
            assert_eq!(certificate.calculate_id(), certificates[0].calculate_id());
        }
    }

    #[test]
    fn it_rejects_a_signer_outside_the_committee() {
        let err = create_timeout_certificate(&committee(&[1, 2, 3]), Epoch(1), NodeHeight(5), &[vote(1), vote(5)])
            .unwrap_err();
        assert!(matches!(err, HotStuffError::InvariantError(_)), "{err}");
    }

    #[test]
    fn it_rejects_a_vote_from_another_epoch() {
        let err = check_timeout_vote_epoch(Epoch(2), &vote(1)).unwrap_err();
        assert!(matches!(err, HotStuffError::InvalidVote { .. }), "{err}");
    }
}
