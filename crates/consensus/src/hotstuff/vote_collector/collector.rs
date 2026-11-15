//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
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

/// Whether a vote at `vote_height` is below the window `[current_height - 1, current_height + MAX_VOTE_VIEW_LOOKAHEAD]`
/// that the collector retains.
pub fn is_before_vote_window(current_height: NodeHeight, vote_height: NodeHeight) -> bool {
    vote_height < current_height.saturating_sub(NodeHeight(1))
}

/// Whether a vote at `vote_height` is too far ahead of `current_height` to be worth keeping.
pub fn exceeds_vote_lookahead(current_height: NodeHeight, vote_height: NodeHeight) -> bool {
    vote_height > current_height.saturating_add(MAX_VOTE_VIEW_LOOKAHEAD)
}

#[derive(Debug, Clone, Copy)]
enum VoteStoreBound {
    /// Discard votes whose height exceeds `current_height + max_lookahead`.
    ViewLookahead(NodeHeight),
    /// Retain at most `max_per_sender` views per signer, evicting a signer's oldest buffered view when a
    /// higher-view vote arrives from that signer.
    PerSender(usize),
}

impl Default for VoteStoreBound {
    fn default() -> Self {
        Self::ViewLookahead(MAX_VOTE_VIEW_LOOKAHEAD)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveVoteOutcome {
    Saved,
    Ignored,
}

#[derive(Clone)]
pub struct VoteCollector<V: Vote> {
    store: Arc<RwLock<VoteStoreInner<V>>>,
}

impl<V: Vote + Display + Clone> VoteCollector<V>
where V::AggregationKey: Clone
{
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(VoteStoreInner::new())),
        }
    }

    pub fn with_per_sender_limit(max_per_sender: usize) -> Self {
        assert!(max_per_sender > 0, "max_per_sender must be non-zero");
        Self {
            store: Arc::new(RwLock::new(VoteStoreInner::with_bound(VoteStoreBound::PerSender(
                max_per_sender,
            )))),
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

        if vote.epoch() != current_epoch || is_before_vote_window(current_height, vote.height()) {
            debug!(
                target: LOG_TARGET,
                "🗑️ Discarding stale or wrong-epoch {} from {}: current view is {}/{}",
                vote,
                sender_vn.address,
                current_epoch,
                current_height
            );
            return Ok(None);
        }

        if let VoteStoreBound::ViewLookahead(max_lookahead) = access_mut.bound &&
            vote.height() > current_height.saturating_add(max_lookahead)
        {
            warn!(
                target: LOG_TARGET,
                "🗑️ Discarding {} from {}: it is more than {} views ahead of our current view {}/{}",
                vote,
                sender_vn.address,
                max_lookahead,
                current_epoch,
                current_height
            );
            return Ok(None);
        }

        let epoch = vote.epoch();
        let height = vote.height();
        let agg_key = vote.aggregation_key();
        let vote_display = vote.to_string();
        if access_mut.save_vote(vote)? == SaveVoteOutcome::Ignored {
            return Ok(None);
        }

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

        let quorum_votes = access_mut.take_accept_votes(epoch, height, &agg_key);
        if quorum_votes.is_some() {
            debug!(
                target: LOG_TARGET,
                "🔥 Received {} from {} ({} of {}). QUORUM!",
                vote_display,
                sender_vn.address,
                accept_power,
                quorum_threshold
            );
        }

        Ok(quorum_votes)
    }

    pub async fn return_votes(&self, votes: Vec<V>) {
        let mut access_mut = self.store.write().await;
        access_mut.restore_unprocessed_quorum(votes);
    }
}

/// Collection of votes indexed by sender leaf hash
type SenderVotesCollection<V> = HashMap<RistrettoPublicKeyBytes, V>;

struct ViewVotes<V: Vote> {
    votes: SenderVotesCollection<V>,
    quorums_reached: Vec<V::AggregationKey>,
}

impl<V: Vote> Default for ViewVotes<V> {
    fn default() -> Self {
        Self {
            votes: HashMap::new(),
            quorums_reached: Vec::new(),
        }
    }
}

#[derive(Default)]
struct VoteStoreInner<V: Vote> {
    bound: VoteStoreBound,
    /// Maps (Epoch,Height) -> per-view votes and formed quorums
    store: BTreeMap<(Epoch, NodeHeight), ViewVotes<V>>,
    /// Under `VoteStoreBound::PerSender`, tracks the `(Epoch, NodeHeight)` views currently held for each signer.
    sender_views: HashMap<RistrettoPublicKeyBytes, BTreeSet<(Epoch, NodeHeight)>>,
}

impl<V: Vote + Display + Clone> VoteStoreInner<V> {
    const VOTE_BYTE_SIZE: usize = size_of::<V>() + size_of::<RistrettoPublicKeyBytes>();

    pub fn new() -> Self {
        Self::with_bound(VoteStoreBound::default())
    }

    fn with_bound(bound: VoteStoreBound) -> Self {
        if let VoteStoreBound::PerSender(max_per_sender) = bound {
            assert!(max_per_sender > 0, "max_per_sender must be non-zero");
        }
        Self {
            bound,
            store: BTreeMap::new(),
            sender_views: HashMap::new(),
        }
    }

    fn clear_votes_before(&mut self, epoch: Epoch, height: NodeHeight) {
        let cutoff = (epoch, height.saturating_sub(NodeHeight(1)));
        let kept = self.store.split_off(&cutoff);
        let dropped = std::mem::replace(&mut self.store, kept);
        if matches!(self.bound, VoteStoreBound::PerSender(_)) {
            for (dropped_view, view_votes) in dropped {
                for pk in view_votes.votes.into_keys() {
                    if let Some(views) = self.sender_views.get_mut(&pk) {
                        views.remove(&dropped_view);
                        if views.is_empty() {
                            self.sender_views.remove(&pk);
                        }
                    }
                }
            }
        }
        self.log_buffer_size();
    }

    /// Save a vote to the store. Returns DuplicateVoteDetected if a differently signed vote for this view by the
    /// sender was already present.
    ///
    /// The vote already held is kept and the incoming one discarded. The first vote may be the honest
    /// one — this node cannot tell which is — and it may already be counted towards a quorum an honest
    /// committee is forming, so dropping it would let an equivocator erase a real vote.
    pub fn save_vote(&mut self, vote: V) -> Result<SaveVoteOutcome, DuplicateVoteDetected<V>> {
        let epoch_height = (vote.epoch(), vote.height());
        let sender_pk = *vote.public_key();

        if let Some(prev_vote) = self.store.get(&epoch_height).and_then(|vv| vv.votes.get(&sender_pk)) {
            let is_same_vote = prev_vote.signature() == vote.signature();
            if is_same_vote {
                debug!(
                    target: LOG_TARGET,
                    "ℹ️  Received identical duplicate vote {}. Ignoring.",
                    vote,
                );
                return Ok(SaveVoteOutcome::Ignored);
            }

            warn!(
                target: LOG_TARGET,
                "❓️ Received duplicate vote for {}. This could be malicious because a validator should only vote once for the same block.",
                vote,
            );
            return Err(DuplicateVoteDetected {
                epoch: vote.epoch(),
                height: vote.height(),
                public_key: sender_pk,
                previous_vote: prev_vote.clone(),
                new_vote: vote,
            });
        }

        if let VoteStoreBound::PerSender(max_per_sender) = self.bound {
            let views = self.sender_views.entry(sender_pk).or_default();
            if views.len() >= max_per_sender {
                let &oldest_view = views
                    .first()
                    .expect("sender_views is non-empty when len >= max_per_sender >= 1");
                if epoch_height < oldest_view {
                    debug!(
                        target: LOG_TARGET,
                        "🗑️ Discarding {} from {}: per-sender buffer ({}) holds newer views starting at {}/{}",
                        vote,
                        sender_pk,
                        max_per_sender,
                        oldest_view.0,
                        oldest_view.1,
                    );
                    return Ok(SaveVoteOutcome::Ignored);
                }
                views.pop_first();
                if let Some(view_votes) = self.store.get_mut(&oldest_view) {
                    view_votes.votes.remove(&sender_pk);
                    if view_votes.votes.is_empty() && view_votes.quorums_reached.is_empty() {
                        self.store.remove(&oldest_view);
                    }
                }
            }
            views.insert(epoch_height);
        }

        self.store
            .entry(epoch_height)
            .or_default()
            .votes
            .insert(sender_pk, vote);
        Ok(SaveVoteOutcome::Saved)
    }

    /// Marks `agg_key` at `(epoch, height)` as having formed a quorum and returns its Accept votes, or `None` if a
    /// quorum was already emitted for `agg_key` at this view.
    ///
    /// All votes at `(epoch, height)` remain stored until [`Self::clear_votes_before`] prunes the view so that a
    /// signer equivocating after the quorum is still detected and votes for sibling forks or `Reject` decisions are
    /// preserved.
    pub fn take_accept_votes(
        &mut self,
        epoch: Epoch,
        height: NodeHeight,
        agg_key: &V::AggregationKey,
    ) -> Option<Vec<V>>
    where
        V::AggregationKey: Clone,
    {
        let epoch_height = (epoch, height);
        let view_votes = self.store.get_mut(&epoch_height)?;
        if view_votes.quorums_reached.contains(agg_key) {
            return None;
        }
        let accept_votes: Vec<V> = view_votes
            .votes
            .values()
            .filter(|vote| vote.decision().is_accept() && vote.aggregation_key() == *agg_key)
            .cloned()
            .collect();
        view_votes.quorums_reached.push(agg_key.clone());
        Some(accept_votes)
    }

    fn restore_unprocessed_quorum(&mut self, votes: Vec<V>) {
        for vote in votes {
            let epoch_height = (vote.epoch(), vote.height());
            let agg_key = vote.aggregation_key();
            if let Some(view_votes) = self.store.get_mut(&epoch_height) {
                view_votes.quorums_reached.retain(|k| *k != agg_key);
            }
            if !self
                .store
                .get(&epoch_height)
                .is_some_and(|vv| vv.votes.contains_key(vote.public_key()))
            {
                let _ = self.save_vote(vote);
            }
        }
    }

    fn votes_for_key_iter(&self, epoch: Epoch, height: NodeHeight) -> Option<impl Iterator<Item = &V>> {
        let epoch_height = (epoch, height);
        let view_votes = self.store.get(&epoch_height)?;
        Some(view_votes.votes.values())
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
            power = power.saturating_add(committee.get_power_by_public_key(vote.public_key()).unwrap_or_default());
        }
        power
    }

    fn log_buffer_size(&self) {
        debug!(
            target: LOG_TARGET,
            "Vote store size: used: >{:.2?}KiB ({} entries)",
            self.store.values()
                .map(|v| v.votes.len() * Self::VOTE_BYTE_SIZE)
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
    fn quorum_is_reached_when_the_signers_power_crosses_the_threshold() {
        let heavy = pubkey(1);
        let lights = [pubkey(2), pubkey(3), pubkey(4)];
        // n = 5 + 1 + 1 + 1 = 8, f = 2, quorum_threshold = 6
        let committee = Committee::new(
            [(heavy, 5), (lights[0], 1), (lights[1], 1), (lights[2], 1)]
                .into_iter()
                .map(|(pk, power)| CommitteeMember {
                    address: pk,
                    public_key: pk,
                    vote_power: VotePower::of(power),
                })
                .collect(),
        );
        assert_eq!(committee.quorum_threshold(), VotePower::of(6));

        let block_id = FixedHash::new([0xAu8; 32]);
        let vote = |public_key| TestVote {
            epoch: Epoch(1),
            height: NodeHeight(1),
            block_id,
            decision: QuorumDecision::Accept,
            sig: ZERO_SIG,
            public_key,
        };
        let power =
            |store: &VoteStoreInner<TestVote>| store.accept_power(Epoch(1), NodeHeight(1), &block_id, &committee);

        // Three of four members, but only 3 of the 8 power.
        let mut store = VoteStoreInner::<TestVote>::new();
        for pk in lights {
            store.save_vote(vote(pk)).unwrap();
        }
        assert_eq!(power(&store), VotePower::of(3));
        assert!(power(&store) < committee.quorum_threshold());

        // One member holding 5 of the 8 power is still short; one more light vote crosses the threshold.
        let mut store = VoteStoreInner::<TestVote>::new();
        store.save_vote(vote(heavy)).unwrap();
        assert!(power(&store) < committee.quorum_threshold());
        store.save_vote(vote(lights[0])).unwrap();
        assert_eq!(power(&store), VotePower::of(6));
        assert!(power(&store) >= committee.quorum_threshold());
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

    #[tokio::test]
    async fn it_discards_votes_before_the_retained_view_window() {
        let collector = VoteCollector::<TestVote>::new();
        let epoch = Epoch(2);
        let current_height = NodeHeight(10);
        let voter = pubkey(1);
        let committee = committee(&[voter, pubkey(2), pubkey(3), pubkey(4)]);

        let mk = |vote_epoch, height| TestVote {
            epoch: vote_epoch,
            height,
            block_id: FixedHash::zero(),
            decision: QuorumDecision::Accept,
            sig: ZERO_SIG,
            public_key: voter,
        };

        // `current_height - 2` and wrong-epoch votes fall outside `[current_height - 1, current_height + lookahead]`.
        collector
            .collect_vote(
                &validator(voter),
                epoch,
                current_height,
                mk(epoch, NodeHeight(8)),
                &committee,
            )
            .await
            .unwrap();
        collector
            .collect_vote(
                &validator(voter),
                epoch,
                current_height,
                mk(Epoch(1), current_height),
                &committee,
            )
            .await
            .unwrap();
        assert!(collector.store.read().await.store.is_empty());

        // `current_height - 1` is the lower boundary kept for NEWVIEW `last_vote` aggregation.
        collector
            .collect_vote(
                &validator(voter),
                epoch,
                current_height,
                mk(epoch, NodeHeight(9)),
                &committee,
            )
            .await
            .unwrap();
        assert_eq!(collector.store.read().await.store.len(), 1);
    }

    #[tokio::test]
    async fn equivocation_after_quorum_is_detected_and_no_second_quorum_is_emitted() {
        let collector = VoteCollector::<TestVote>::new();
        let epoch = Epoch(1);
        let height = NodeHeight(5);
        let voters = [pubkey(1), pubkey(2), pubkey(3), pubkey(4)];
        let committee = committee(&voters);
        let block_a = FixedHash::new([0xAu8; 32]);
        let block_b = FixedHash::new([0xBu8; 32]);

        let mk = |public_key, block_id, sig| TestVote {
            epoch,
            height,
            block_id,
            decision: QuorumDecision::Accept,
            sig,
            public_key,
        };

        for voter in &voters[..2] {
            let res = collector
                .collect_vote(
                    &validator(*voter),
                    epoch,
                    height,
                    mk(*voter, block_a, ZERO_SIG),
                    &committee,
                )
                .await
                .unwrap();
            assert!(res.is_none());
        }
        let quorum = collector
            .collect_vote(
                &validator(voters[2]),
                epoch,
                height,
                mk(voters[2], block_a, ZERO_SIG),
                &committee,
            )
            .await
            .unwrap();
        assert_eq!(quorum.unwrap().len(), 3);

        // A late fourth vote for the same block does not emit a duplicate quorum.
        let late = collector
            .collect_vote(
                &validator(voters[3]),
                epoch,
                height,
                mk(voters[3], block_a, ZERO_SIG),
                &committee,
            )
            .await
            .unwrap();
        assert!(late.is_none());

        // A quorum signer that subsequently votes for a conflicting block at the same view is still caught.
        let conflicting = mk(
            voters[0],
            block_b,
            SchnorrSignatureBytes::new([1u8; 32].into(), Scalar32Bytes::zero()),
        );
        let err = collector
            .collect_vote(&validator(voters[0]), epoch, height, conflicting.clone(), &committee)
            .await
            .unwrap_err();
        assert_eq!(err.public_key, voters[0]);
        assert_eq!(err.new_vote, conflicting);
    }

    #[tokio::test]
    async fn returning_votes_for_an_unknown_block_preserves_other_votes_and_allows_recollection() {
        let collector = VoteCollector::<TestVote>::new();
        let epoch = Epoch(1);
        let height = NodeHeight(3);
        let voters = [pubkey(1), pubkey(2), pubkey(3), pubkey(4)];
        let committee = committee(&voters);
        let block_a = FixedHash::new([0xAu8; 32]);

        let mk = |public_key, decision, sig| TestVote {
            epoch,
            height,
            block_id: block_a,
            decision,
            sig,
            public_key,
        };

        // Voter 4 casts a Reject vote before voters 1..=3 form an Accept quorum on an unknown block.
        collector
            .collect_vote(
                &validator(voters[3]),
                epoch,
                height,
                mk(voters[3], QuorumDecision::Reject, ZERO_SIG),
                &committee,
            )
            .await
            .unwrap();
        for voter in &voters[..2] {
            collector
                .collect_vote(
                    &validator(*voter),
                    epoch,
                    height,
                    mk(*voter, QuorumDecision::Accept, ZERO_SIG),
                    &committee,
                )
                .await
                .unwrap();
        }
        let quorum_votes = collector
            .collect_vote(
                &validator(voters[2]),
                epoch,
                height,
                mk(voters[2], QuorumDecision::Accept, ZERO_SIG),
                &committee,
            )
            .await
            .unwrap()
            .unwrap();
        collector.return_votes(quorum_votes).await;

        // Voter 4's Reject vote is still retained, so an Accept vote from voter 4 is caught as equivocation.
        let flip = mk(
            voters[3],
            QuorumDecision::Accept,
            SchnorrSignatureBytes::new([2u8; 32].into(), Scalar32Bytes::zero()),
        );
        let dup = collector
            .collect_vote(&validator(voters[3]), epoch, height, flip, &committee)
            .await
            .unwrap_err();
        assert_eq!(dup.public_key, voters[3]);

        // Re-evaluating one of the returned Accept votes once the block arrives emits the quorum again.
        let recollected = collector
            .collect_vote(
                &validator(voters[0]),
                epoch,
                height,
                mk(voters[0], QuorumDecision::Accept, ZERO_SIG),
                &committee,
            )
            .await
            .unwrap();
        assert_eq!(recollected.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn per_sender_collector_reaches_quorum_far_beyond_proposal_lookahead() {
        let collector = VoteCollector::<TestVote>::with_per_sender_limit(8);
        let epoch = Epoch(1);
        let stuck_height = NodeHeight(10);
        let far_timeout_height = stuck_height + MAX_VOTE_VIEW_LOOKAHEAD + NodeHeight(200);
        let voters = [pubkey(1), pubkey(2), pubkey(3), pubkey(4)];
        let committee = committee(&voters);

        let mk = |pk| TestVote {
            epoch,
            height: far_timeout_height,
            block_id: FixedHash::zero(),
            decision: QuorumDecision::Accept,
            sig: ZERO_SIG,
            public_key: pk,
        };

        for &voter in &voters[..2] {
            assert!(
                collector
                    .collect_vote(&validator(voter), epoch, stuck_height, mk(voter), &committee)
                    .await
                    .unwrap()
                    .is_none()
            );
        }

        let quorum = collector
            .collect_vote(&validator(voters[2]), epoch, stuck_height, mk(voters[2]), &committee)
            .await
            .unwrap()
            .expect("third vote should complete quorum beyond lookahead window");
        assert_eq!(quorum.len(), 3);
        // Votes remain retained at `far_timeout_height` so a fourth vote does not emit a duplicate quorum, and are
        // pruned once `current_height` advances past `far_timeout_height + 1`.
        assert!(
            collector
                .collect_vote(&validator(voters[3]), epoch, stuck_height, mk(voters[3]), &committee)
                .await
                .unwrap()
                .is_none()
        );
        let next_height = far_timeout_height + NodeHeight(2);
        collector
            .collect_vote(
                &validator(voters[0]),
                epoch,
                next_height,
                TestVote {
                    height: next_height,
                    ..mk(voters[0])
                },
                &committee,
            )
            .await
            .unwrap();
        let guard = collector.store.read().await;
        assert!(
            guard
                .sender_views
                .values()
                .all(|views| !views.contains(&(epoch, far_timeout_height)))
        );
    }

    #[tokio::test]
    async fn per_sender_limit_caps_spammer_evicts_oldest_and_preserves_other_signers() {
        let limit = 4;
        let collector = VoteCollector::<TestVote>::with_per_sender_limit(limit);
        let epoch = Epoch(1);
        let current_height = NodeHeight(5);
        let honest1 = pubkey(1);
        let honest2 = pubkey(2);
        let spammer = pubkey(3);
        let absent = pubkey(4);
        let committee = committee(&[honest1, honest2, spammer, absent]);

        let mk = |pk, height| TestVote {
            epoch,
            height: NodeHeight(height),
            block_id: FixedHash::zero(),
            decision: QuorumDecision::Accept,
            sig: ZERO_SIG,
            public_key: pk,
        };

        // Honest members buffer a vote at view 40 (well past MAX_VOTE_VIEW_LOOKAHEAD).
        collector
            .collect_vote(&validator(honest1), epoch, current_height, mk(honest1, 40), &committee)
            .await
            .unwrap();

        // Spammer sends 50 distinct future views (10..60).
        for h in 10..60 {
            collector
                .collect_vote(&validator(spammer), epoch, current_height, mk(spammer, h), &committee)
                .await
                .unwrap();
        }

        {
            let guard = collector.store.read().await;
            let spammer_views = &guard.sender_views[&spammer];
            assert_eq!(spammer_views.len(), limit);
            // Only the 4 highest views (56..60) remain for the spammer.
            let expected: BTreeSet<_> = (56..60).map(|h| (epoch, NodeHeight(h))).collect();
            assert_eq!(spammer_views, &expected);
            // Empty buckets from evicted spammer heights were removed, leaving honest1's view 40 + 4 spammer views.
            assert_eq!(guard.store.len(), 1 + limit);
        }

        // A stale below-floor vote from the spammer (view 20 < 56) is ignored without evicting newer views.
        collector
            .collect_vote(&validator(spammer), epoch, current_height, mk(spammer, 20), &committee)
            .await
            .unwrap();
        assert!(
            !collector
                .store
                .read()
                .await
                .store
                .contains_key(&(epoch, NodeHeight(20)))
        );

        // Honest members still reach quorum at view 58 using the spammer's retained vote, and honest1's view 40 was
        // untouched.
        collector
            .collect_vote(&validator(honest1), epoch, current_height, mk(honest1, 58), &committee)
            .await
            .unwrap();
        let quorum = collector
            .collect_vote(&validator(honest2), epoch, current_height, mk(honest2, 58), &committee)
            .await
            .unwrap()
            .expect("quorum should form at view 58");
        assert_eq!(quorum.len(), 3);

        // Advancing current_height past 58 clears old views and releases per-sender tracking cleanly.
        collector
            .collect_vote(&validator(honest1), epoch, NodeHeight(60), mk(honest1, 62), &committee)
            .await
            .unwrap();
        let guard = collector.store.read().await;
        assert_eq!(guard.sender_views.get(&spammer).map(|s| s.len()).unwrap_or(0), 1);
    }

    #[test]
    #[should_panic(expected = "max_per_sender must be non-zero")]
    fn with_per_sender_limit_rejects_zero() {
        let _collector = VoteCollector::<TestVote>::with_per_sender_limit(0);
    }
}
