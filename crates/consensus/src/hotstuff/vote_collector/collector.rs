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
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tokio::sync::RwLock;

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::vote_collector";

/// How many views ahead of our own a vote may be and still be kept.
///
/// Votes are held until their view is reached and the view a vote names is chosen by its sender, so this
/// window is what bounds the store. A node lagging the committee by more than this window must sync: it
/// reaches those views by importing the blocks, not by certifying them from buffered votes.
const MAX_VOTE_VIEW_LOOKAHEAD: NodeHeight = NodeHeight(20);

/// Whether a vote at `vote_height` is too far ahead of `current_height` to be worth keeping.
pub fn exceeds_vote_lookahead(current_height: NodeHeight, vote_height: NodeHeight) -> bool {
    vote_height > current_height.saturating_add(MAX_VOTE_VIEW_LOOKAHEAD)
}

#[derive(Clone)]
pub struct VoteCollector<V: Vote> {
    store: Arc<RwLock<VoteStoreInner<V>>>,
}

impl<V: Vote + Display + Clone> VoteCollector<V> {
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
    ) -> Result<Option<Vec<V>>, DuplicateVoteDetected<V>> {
        let mut access_mut = self.store.write().await;
        access_mut.clear_votes_before(current_epoch, current_height);

        if exceeds_vote_lookahead(current_height, vote.height()) {
            warn!(
                target: LOG_TARGET,
                "🗑️ Discarding {} from {}: it is more than {} views ahead of our current view {}/{}",
                vote,
                sender_vn.address,
                MAX_VOTE_VIEW_LOOKAHEAD,
                current_epoch,
                current_height
            );
            return Ok(None);
        }

        let epoch = vote.epoch();
        let height = vote.height();
        let agg_key = vote.aggregation_key();
        let vote_display = vote.to_string();
        access_mut.save_vote(vote)?;

        let accept_power = access_mut.accept_power(epoch, height, &agg_key, committee);

        // We only generate the next qc once when we have a quorum of votes. Any votes received after this
        // are not included in the QC.
        let quorum_threshold = committee.quorum_threshold();
        if accept_power < quorum_threshold {
            debug!(
                target: LOG_TARGET,
                "🔥 Received {} from {} ({} of {}).",
                vote_display,
                sender_vn.address,
                accept_power,
                quorum_threshold
            );
            return Ok(None);
        }

        debug!(
            target: LOG_TARGET,
            "🔥 Received {} from {} ({} of {}). QUORUM!",
            vote_display,
            sender_vn.address,
            accept_power,
            quorum_threshold
        );

        Ok(access_mut.take_accept_votes(epoch, height, &agg_key))
    }

    pub async fn return_votes(&self, votes: Vec<V>) {
        let mut access_mut = self.store.write().await;
        for vote in votes {
            if let Err(err) = access_mut.save_vote(vote) {
                // To panic or not to panic? That is the question...
                error!(
                    target: LOG_TARGET,
                    "❌: BUG DETECTED: a duplicate vote on returned votes should not be possible: {}", err,
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

impl<V: Vote + Display + Clone> VoteStoreInner<V> {
    const VOTE_BYTE_SIZE: usize = size_of::<V>() + size_of::<RistrettoPublicKeyBytes>();

    pub fn new() -> Self {
        Self { store: BTreeMap::new() }
    }

    fn clear_votes_before(&mut self, epoch: Epoch, height: NodeHeight) {
        self.store = self.store.split_off(&(epoch, height.saturating_sub(NodeHeight(1))));
        self.log_buffer_size();
    }

    /// Save a vote to the store. Returns DuplicateVoteDetected if a differently signed vote for this view by the
    /// sender was already present.
    ///
    /// The vote already held is kept and the incoming one discarded. The first vote may be the honest
    /// one — this node cannot tell which is — and it may already be counted towards a quorum an honest
    /// committee is forming, so dropping it would let an equivocator erase a real vote.
    pub fn save_vote(&mut self, vote: V) -> Result<(), DuplicateVoteDetected<V>> {
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
            return Err(DuplicateVoteDetected {
                epoch: vote.epoch(),
                height: vote.height(),
                public_key: *vote.public_key(),
                previous_vote: prev_vote.clone(),
                new_vote: vote,
            });
        }

        view_votes_mut.insert(*vote.public_key(), vote);
        Ok(())
    }

    /// Removes every vote at `(epoch, height)` and returns the Accept votes for `agg_key`.
    pub fn take_accept_votes(
        &mut self,
        epoch: Epoch,
        height: NodeHeight,
        agg_key: &V::AggregationKey,
    ) -> Option<Vec<V>> {
        let epoch_height = (epoch, height);
        // Once a block is decided at this height, votes for any losing fork at the same height can be dropped too.
        let votes = self.store.remove(&epoch_height)?;
        Some(
            votes
                .into_values()
                .filter(|vote| vote.decision().is_accept() && vote.aggregation_key() == *agg_key)
                .collect(),
        )
    }

    fn votes_for_key_iter(&self, epoch: Epoch, height: NodeHeight) -> Option<impl Iterator<Item = &V>> {
        let epoch_height = (epoch, height);
        let votes = self.store.get(&epoch_height)?;
        Some(votes.values())
    }

    /// The vote power of the Accept votes for `agg_key` at `(epoch, height)`.
    ///
    /// Reject votes are held so that a signer who also sends an Accept vote for the view is caught
    /// equivocating, but they count towards no quorum: every certificate attests to an Accept.
    pub fn accept_power<TAddr: NodeAddressable>(
        &self,
        epoch: Epoch,
        height: NodeHeight,
        agg_key: &V::AggregationKey,
        committee: &Committee<TAddr>,
    ) -> VotePower {
        let Some(votes_iter) = self.votes_for_key_iter(epoch, height) else {
            // Soft invariant protection - technically, this should not happen but the correct power is returned
            // regardless i.e. 0 votes
            error!(
                target: LOG_TARGET,
                "INVARIANT: accept_power: no votes for vote ({}/{}). Votes should have been collected before calling this function",
                epoch, height,
            );
            return VotePower::zero();
        };
        let mut power = VotePower::zero();
        for vote in votes_iter.filter(|vote| vote.decision().is_accept() && vote.aggregation_key() == *agg_key) {
            power += committee.get_power_by_public_key(vote.public_key()).unwrap_or_default();
        }
        power
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

/// A second, differently signed vote from a signer that already has one in this view's bucket.
///
/// Whether this is equivocation depends on what the two votes attest to, which the caller decides:
/// signing draws a fresh nonce, so one signer can produce many valid signatures over one message,
/// and a vote type whose preimage carries nothing beyond the view it is bucketed under cannot
/// express a conflict at all.
#[derive(Debug, Clone, thiserror::Error)]
#[error("Duplicate vote detected at epoch {epoch}, height {height} from {public_key}")]
pub struct DuplicateVoteDetected<V: Vote> {
    pub epoch: Epoch,
    pub height: NodeHeight,
    pub public_key: RistrettoPublicKeyBytes,
    pub previous_vote: V,
    pub new_vote: V,
}

#[cfg(test)]
mod tests {
    use tari_consensus_types::{SignedMessage, ToSignatureMessage};
    use tari_ootle_common_types::{SubstateAddress, committee::CommitteeMember};
    use tari_sidechain::{FixedHash, QuorumDecision};
    use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, Scalar32Bytes, SchnorrSignatureBytes};

    use super::*;

    const ZERO_SIG: SchnorrSignatureBytes = SchnorrSignatureBytes::zero();
    const ZERO_PUBKEY: RistrettoPublicKeyBytes = RistrettoPublicKeyBytes::zero();

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestVote {
        epoch: Epoch,
        height: NodeHeight,
        block_id: FixedHash,
        decision: QuorumDecision,
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
        type AggregationKey = FixedHash;

        fn epoch(&self) -> Epoch {
            self.epoch
        }

        fn height(&self) -> NodeHeight {
            self.height
        }

        fn decision(&self) -> QuorumDecision {
            self.decision
        }

        fn aggregation_key(&self) -> FixedHash {
            self.block_id
        }
    }

    fn pubkey(byte: u8) -> RistrettoPublicKeyBytes {
        RistrettoPublicKeyBytes::from_bytes(&[byte; 32]).unwrap()
    }

    fn validator(public_key: RistrettoPublicKeyBytes) -> ValidatorNode<RistrettoPublicKeyBytes> {
        ValidatorNode {
            address: public_key,
            public_key,
            shard_key: SubstateAddress::zero(),
            start_epoch: Epoch(0),
            end_epoch: None,
            fee_claim_public_key: public_key,
            vote_power: VotePower::of(1),
        }
    }

    fn committee(public_keys: &[RistrettoPublicKeyBytes]) -> Committee<RistrettoPublicKeyBytes> {
        Committee::new(
            public_keys
                .iter()
                .map(|pk| CommitteeMember {
                    address: *pk,
                    public_key: *pk,
                    vote_power: VotePower::of(1),
                })
                .collect(),
        )
    }

    #[test]
    fn it_saves_a_new_vote() {
        let mut store = VoteStoreInner::<TestVote>::new();
        let vote = TestVote {
            epoch: Epoch(1),
            height: NodeHeight(1),
            block_id: FixedHash::zero(),
            decision: QuorumDecision::Accept,
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
            block_id: FixedHash::zero(),
            decision: QuorumDecision::Accept,
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
            block_id: FixedHash::zero(),
            decision: QuorumDecision::Accept,
            public_key: ZERO_PUBKEY,
            sig: SchnorrSignatureBytes::new([1u8; 32].into(), Scalar32Bytes::zero()),
        };

        // Try to save the a different vote from the same public key and epoch/height - should error
        store.save_vote(vote).unwrap_err();
    }

    #[test]
    fn a_foreign_block_vote_does_not_count_towards_a_block_quorum() {
        let mut store = VoteStoreInner::<TestVote>::new();
        let epoch = Epoch(1);
        let height = NodeHeight(1);

        let block_a = FixedHash::new([0xAu8; 32]);
        let block_phantom = FixedHash::new([0xBu8; 32]);

        let honest1 = pubkey(1);
        let honest2 = pubkey(2);
        let byzantine = pubkey(3);
        let absent = pubkey(4);

        // n = 4, quorum_threshold = 3
        let committee = committee(&[honest1, honest2, byzantine, absent]);
        assert_eq!(committee.quorum_threshold(), VotePower::of(3));

        let mk = |pk, block_id| TestVote {
            epoch,
            height,
            block_id,
            decision: QuorumDecision::Accept,
            sig: ZERO_SIG,
            public_key: pk,
        };

        // Two honest Accept votes for block A plus one Byzantine Accept vote for a phantom block, all at the same
        // (epoch, height).
        store.save_vote(mk(honest1, block_a)).unwrap();
        store.save_vote(mk(honest2, block_a)).unwrap();
        store.save_vote(mk(byzantine, block_phantom)).unwrap();

        // Block A only has 2 of the required 3 power and does not reach a quorum.
        assert_eq!(
            store.accept_power(epoch, height, &block_a, &committee),
            VotePower::of(2)
        );

        // The Byzantine phantom-block vote never folds into block A's votes.
        let votes_a = store.take_accept_votes(epoch, height, &block_a).unwrap();
        assert_eq!(votes_a.len(), 2);
        assert!(votes_a.iter().all(|v| v.block_id == block_a));
    }

    /// An equivocator must not be able to cancel a vote it has already cast. `save_vote` holds one vote
    /// per (view, signer), so if the conflicting vote evicted the stored one, a Byzantine member could
    /// take its own power back out of a quorum an honest committee was about to reach.
    #[test]
    fn an_equivocating_vote_does_not_erase_the_vote_already_held() {
        let mut store = VoteStoreInner::<TestVote>::new();
        let epoch = Epoch(1);
        let height = NodeHeight(1);

        let block_a = FixedHash::new([0xAu8; 32]);
        let block_b = FixedHash::new([0xBu8; 32]);

        let honest1 = pubkey(1);
        let honest2 = pubkey(2);
        let byzantine = pubkey(3);
        let absent = pubkey(4);

        // n = 4, quorum_threshold = 3: the two honest votes alone are one short.
        let committee = committee(&[honest1, honest2, byzantine, absent]);
        assert_eq!(committee.quorum_threshold(), VotePower::of(3));

        let mk = |pk, block_id, sig| TestVote {
            epoch,
            height,
            block_id,
            decision: QuorumDecision::Accept,
            sig,
            public_key: pk,
        };

        let first = mk(byzantine, block_a, ZERO_SIG);
        store.save_vote(first.clone()).unwrap();
        store.save_vote(mk(honest1, block_a, ZERO_SIG)).unwrap();

        // The same signer now votes for a different block at the same view.
        let second = mk(
            byzantine,
            block_b,
            SchnorrSignatureBytes::new([1u8; 32].into(), Scalar32Bytes::zero()),
        );
        let equivocation = store.save_vote(second.clone()).unwrap_err();
        assert_eq!(equivocation.public_key, byzantine);
        assert_eq!(equivocation.previous_vote, first);
        assert_eq!(equivocation.new_vote, second);

        store.save_vote(mk(honest2, block_a, ZERO_SIG)).unwrap();

        assert_eq!(
            store.accept_power(epoch, height, &block_a, &committee),
            VotePower::of(3)
        );

        let votes = store.take_accept_votes(epoch, height, &block_a).unwrap();
        assert_eq!(votes.len(), 3);
        assert!(votes.iter().any(|v| v.public_key == byzantine && v.block_id == block_a));
    }

    #[tokio::test]
    async fn a_reject_quorum_forms_no_certificate() {
        let collector = VoteCollector::<TestVote>::new();
        let epoch = Epoch(1);
        let height = NodeHeight(1);
        let voters = [pubkey(1), pubkey(2), pubkey(3), pubkey(4)];
        let committee = committee(&voters);

        for voter in voters {
            let vote = TestVote {
                epoch,
                height,
                block_id: FixedHash::zero(),
                decision: QuorumDecision::Reject,
                sig: ZERO_SIG,
                public_key: voter,
            };
            let quorum = collector
                .collect_vote(&validator(voter), epoch, height, vote, &committee)
                .await
                .unwrap();
            assert!(quorum.is_none());
        }
    }

    /// A Reject vote is held, so a signer that also sends an Accept vote for the same block is caught.
    #[tokio::test]
    async fn an_accept_after_a_reject_for_the_same_block_is_detected() {
        let collector = VoteCollector::<TestVote>::new();
        let epoch = Epoch(1);
        let height = NodeHeight(1);
        let voter = pubkey(1);
        let committee = committee(&[voter, pubkey(2), pubkey(3), pubkey(4)]);

        let mk = |decision, sig| TestVote {
            epoch,
            height,
            block_id: FixedHash::zero(),
            decision,
            sig,
            public_key: voter,
        };

        let reject = mk(QuorumDecision::Reject, ZERO_SIG);
        collector
            .collect_vote(&validator(voter), epoch, height, reject.clone(), &committee)
            .await
            .unwrap();

        let accept = mk(
            QuorumDecision::Accept,
            SchnorrSignatureBytes::new([1u8; 32].into(), Scalar32Bytes::zero()),
        );
        let duplicate = collector
            .collect_vote(&validator(voter), epoch, height, accept.clone(), &committee)
            .await
            .unwrap_err();
        assert_eq!(duplicate.previous_vote, reject);
        assert_eq!(duplicate.new_vote, accept);
    }

    #[tokio::test]
    async fn it_discards_votes_too_far_ahead_of_the_current_view() {
        let collector = VoteCollector::<TestVote>::new();
        let epoch = Epoch(1);
        let current_height = NodeHeight(10);
        let voter = pubkey(1);
        let committee = committee(&[voter, pubkey(2), pubkey(3), pubkey(4)]);

        let mk = |height| TestVote {
            epoch,
            height,
            block_id: FixedHash::zero(),
            decision: QuorumDecision::Accept,
            sig: ZERO_SIG,
            public_key: voter,
        };

        let at_limit = current_height + MAX_VOTE_VIEW_LOOKAHEAD;
        collector
            .collect_vote(&validator(voter), epoch, current_height, mk(at_limit), &committee)
            .await
            .unwrap();
        assert_eq!(collector.store.read().await.store.len(), 1);

        collector
            .collect_vote(
                &validator(voter),
                epoch,
                current_height,
                mk(at_limit + NodeHeight(1)),
                &committee,
            )
            .await
            .unwrap();
        assert_eq!(collector.store.read().await.store.len(), 1);
    }
}
