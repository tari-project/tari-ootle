//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    sync::Arc,
};

use log::*;
use tari_consensus_types::Vote;
use tari_ootle_common_types::{Epoch, NodeAddressable, NodeHeight, VotePower, committee::Committee};
use tari_ootle_storage::global::models::ValidatorNode;
use tari_sidechain::QuorumDecision;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tokio::sync::RwLock;

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::vote_collector";

#[derive(Clone)]
pub struct VoteCollector<V: Vote> {
    store: Arc<RwLock<VoteStoreInner<V>>>,
}

impl<V: Vote + Display> VoteCollector<V> {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(VoteStoreInner::new())),
        }
    }

    pub async fn collect_vote<TAddr: NodeAddressable>(
        &self,
        sender_vn: &ValidatorNode<TAddr>,
        current_epoch: Epoch,
        current_height: NodeHeight,
        vote: V,
        committee: &Committee<TAddr>,
    ) -> Result<Option<(Vec<V>, QuorumDecision)>, VoteEquivocationDetected<V>> {
        let mut access_mut = self.store.write().await;
        access_mut.clear_votes_before(current_epoch, current_height);
        let epoch = vote.epoch();
        let height = vote.height();
        let vote_display = vote.to_string();
        access_mut.save_vote(vote)?;

        let threshold_decision = access_mut.calculate_threshold_decision(epoch, height, committee);

        let quorum_threshold = committee.quorum_threshold();
        let Some(quorum_decision) = threshold_decision.decision else {
            debug!(
                target: LOG_TARGET,
                "🔥 Received {} from {} ({} of {}).",
                vote_display,
                sender_vn.address,
                threshold_decision.total_power,
                quorum_threshold
            );
            return Ok(None);
        };

        // We only generate the next qc once when we have a quorum of votes. Any votes received after this
        // are not included in the QC.
        if threshold_decision.total_power < quorum_threshold {
            debug!(
                target: LOG_TARGET,
                "🔥 Received {} from {} ({} of {}).",
                vote_display,
                sender_vn.address,
                threshold_decision.total_power,
                quorum_threshold
            );
            return Ok(None);
        }

        debug!(
            target: LOG_TARGET,
            "🔥 Received {} from {} ({} of {}). QUORUM!",
            vote_display,
            sender_vn.address,
            threshold_decision.total_power,
            quorum_threshold
        );

        let votes = access_mut.take_votes_with_decision(epoch, height, quorum_decision);

        Ok(votes.map(|v| (v, quorum_decision)))
    }

    pub async fn return_votes(&self, votes: Vec<V>) {
        let mut access_mut = self.store.write().await;
        for vote in votes {
            if let Err(err) = access_mut.save_vote(vote) {
                // To panic or not to panic? That is the question...
                error!(
                    target: LOG_TARGET,
                    "❌: BUG DETECTED: EQUIVOCATION on returned votes should not be possible: {}", err,
                );
            }
        }
    }
}

/// Collection of votes indexed by sender leaf hash
type SenderVotesCollection<V> = HashMap<RistrettoPublicKeyBytes, V>;

#[derive(Debug, Default)]
struct VoteStoreInner<V: Vote> {
    /// Maps (Epoch,Height) -> map (key -> map (sender leaf hash -> vote))
    /// The key is some type that uniquely keys the vote e.g BlockId or (Epoch,Height)
    store: BTreeMap<(Epoch, NodeHeight), SenderVotesCollection<V>>,
}

impl<V: Vote + Display> VoteStoreInner<V> {
    const VOTE_BYTE_SIZE: usize = size_of::<V>() + size_of::<RistrettoPublicKeyBytes>();

    pub fn new() -> Self {
        Self { store: BTreeMap::new() }
    }

    fn clear_votes_before(&mut self, epoch: Epoch, height: NodeHeight) {
        self.store = self.store.split_off(&(epoch, height.saturating_sub(NodeHeight(1))));
        self.log_buffer_size();
    }

    /// Save a vote to the store. Returns VoteEquivocationDetected if a previous vote for the block by the sender was
    /// already present.
    pub fn save_vote(&mut self, vote: V) -> Result<(), VoteEquivocationDetected<V>> {
        let epoch_height = (vote.epoch(), vote.height());
        let view_votes_mut = self.store.entry(epoch_height).or_default();
        if let Some(prev_vote) = view_votes_mut.get(vote.public_key()) {
            let is_same_vote = prev_vote.signature() == vote.signature();
            if is_same_vote {
                debug!(
                    target: LOG_TARGET,
                    "ℹ️  Received identical duplicate vote {}. Ignoring.",
                    vote,
                );
                return Ok(());
            }

            warn!(
                target: LOG_TARGET,
                "❓️ Received duplicate vote for {}. This could be malicious because a validator should only vote once for the same block.",
                vote,
            );
            let prev_vote = view_votes_mut.remove(vote.public_key()).expect("Vote is present");
            // We already have a vote for this block from this sender
            return Err(VoteEquivocationDetected {
                epoch: vote.epoch(),
                height: vote.height(),
                public_key: *vote.public_key(),
                previous_vote: prev_vote,
                new_vote: vote,
            });
        }

        view_votes_mut.insert(*vote.public_key(), vote);
        Ok(())
    }

    pub fn take_votes_with_decision(
        &mut self,
        epoch: Epoch,
        height: NodeHeight,
        decision: QuorumDecision,
    ) -> Option<Vec<V>> {
        let epoch_height = (epoch, height);
        // Take all votes, discarding any other votes for this epoch/height that do not match the decision
        let votes = self.store.remove(&epoch_height)?;
        Some(votes.into_values().filter(|vote| vote.decision() == decision).collect())
    }

    fn votes_for_key_iter(&self, epoch: Epoch, height: NodeHeight) -> Option<impl Iterator<Item = &V>> {
        let epoch_height = (epoch, height);
        let votes = self.store.get(&epoch_height)?;
        Some(votes.values())
    }

    pub fn calculate_threshold_decision<TAddr: NodeAddressable>(
        &self,
        epoch: Epoch,
        height: NodeHeight,
        committee: &Committee<TAddr>,
    ) -> ThresholdDecision {
        let Some(votes_iter) = self.votes_for_key_iter(epoch, height) else {
            // Soft invariant protection - technically, this should not happen but the correct ThresholdDecision is
            // returned regardless i.e. 0 votes
            error!(
                target: LOG_TARGET,
                "INVARIANT: calculate_threshold_decision: no votes for vote ({}/{}). Votes should have been collected before calling this function",
                epoch, height,
            );

            return ThresholdDecision {
                total_power: VotePower::zero(),
                decision: None,
            };
        };
        let mut count_accept = VotePower::zero();
        let mut count_reject = VotePower::zero();
        for vote in votes_iter {
            let power = committee.get_power_by_public_key(vote.public_key()).unwrap_or_default();
            match vote.decision() {
                QuorumDecision::Accept => count_accept += power,
                QuorumDecision::Reject => count_reject += power,
            }
        }

        let quorum_threshold = committee.quorum_threshold();
        if count_accept >= quorum_threshold {
            return ThresholdDecision {
                total_power: count_accept,
                decision: Some(QuorumDecision::Accept),
            };
        }
        if count_reject >= quorum_threshold {
            return ThresholdDecision {
                total_power: count_reject,
                decision: Some(QuorumDecision::Reject),
            };
        }

        ThresholdDecision {
            total_power: count_accept + count_reject,
            decision: None,
        }
    }

    fn log_buffer_size(&self) {
        debug!(
            target: LOG_TARGET,
            "Vote store size: used: >{:.2?}KiB ({} entries)",
            self.store.values()
                .map(|v| v.len() * Self::VOTE_BYTE_SIZE)
                .sum::<usize>() as f32 / 1024f32,
            self.store.len(),
        );
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ThresholdDecision {
    pub decision: Option<QuorumDecision>,
    pub total_power: VotePower,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("Vote equivocation detected at epoch {epoch}, height {height} from {public_key}")]
pub struct VoteEquivocationDetected<V: Vote> {
    pub epoch: Epoch,
    pub height: NodeHeight,
    pub public_key: RistrettoPublicKeyBytes,
    pub previous_vote: V,
    pub new_vote: V,
}

#[cfg(test)]
mod tests {
    use tari_common_types::types::FixedHash;
    use tari_consensus_types::{SignedMessage, ToSignatureMessage};
    use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, Scalar32Bytes, SchnorrSignatureBytes};

    use super::*;

    const ZERO_SIG: SchnorrSignatureBytes = SchnorrSignatureBytes::zero();
    const ZERO_PUBKEY: RistrettoPublicKeyBytes = RistrettoPublicKeyBytes::zero();

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct TestVote {
        epoch: Epoch,
        height: NodeHeight,
        sig: SchnorrSignatureBytes,
        public_key: RistrettoPublicKeyBytes,
    }

    impl Display for TestVote {
        fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            Ok(())
        }
    }

    impl ToSignatureMessage for TestVote {
        fn to_signature_message(&self) -> FixedHash {
            FixedHash::zero()
        }
    }

    impl SignedMessage for TestVote {
        fn signature(&self) -> &SchnorrSignatureBytes {
            &self.sig
        }

        fn public_key(&self) -> &RistrettoPublicKeyBytes {
            &self.public_key
        }
    }

    impl Vote for TestVote {
        fn epoch(&self) -> Epoch {
            self.epoch
        }

        fn height(&self) -> NodeHeight {
            self.height
        }

        fn decision(&self) -> QuorumDecision {
            QuorumDecision::Accept
        }
    }

    #[test]
    fn it_saves_a_new_vote() {
        let mut store = VoteStoreInner::<TestVote>::new();
        let vote = TestVote {
            epoch: Epoch(1),
            height: NodeHeight(1),
            sig: ZERO_SIG,
            public_key: ZERO_PUBKEY,
        };
        store
            .save_vote(vote.clone())
            .expect("Expected to save a new vote successfully");
    }

    #[test]
    fn it_detects_a_duplicate_vote() {
        let mut store = VoteStoreInner::<TestVote>::new();
        let vote = TestVote {
            epoch: Epoch(1),
            height: NodeHeight(1),
            sig: ZERO_SIG,
            public_key: ZERO_PUBKEY,
        };
        store
            .save_vote(vote.clone())
            .expect("Expected to save a new vote successfully");

        // Try to save the same vote again - exact same vote is OK
        store.save_vote(vote).unwrap();

        let vote = TestVote {
            epoch: Epoch(1),
            height: NodeHeight(1),
            public_key: ZERO_PUBKEY,
            sig: SchnorrSignatureBytes::new([1u8; 32].into(), Scalar32Bytes::zero()),
        };

        // Try to save the a different vote from the same public key and epoch/height - should error
        store.save_vote(vote).unwrap_err();
    }
}
