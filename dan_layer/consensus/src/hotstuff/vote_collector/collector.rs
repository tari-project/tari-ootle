//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    sync::Arc,
};

use log::*;
use tari_common_types::types::FixedHash;
use tari_consensus_types::Vote;
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_sidechain::QuorumDecision;
use tokio::sync::RwLock;

const LOG_TARGET: &str = "tari::consensus::hotstuff::vote_collector";

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

    pub async fn collect_vote(
        &self,
        current_epoch: Epoch,
        current_height: NodeHeight,
        sender_hash: FixedHash,
        vote: V,
        quorum_threshold: usize,
    ) -> Option<(Vec<V>, QuorumDecision)> {
        let mut access_mut = self.store.write().await;
        access_mut.clear_votes_before(current_epoch, current_height);
        let epoch = vote.epoch();
        let height = vote.height();
        let key = vote.key();
        let vote_display = vote.to_string();
        if !access_mut.save_vote(sender_hash, vote) {
            // We already have a vote for this block from this sender
            return None;
        }

        let threshold_decision = access_mut.calculate_threshold_decision(epoch, height, &key, quorum_threshold);

        let Some(quorum_decision) = threshold_decision.decision else {
            info!(
                target: LOG_TARGET,
                "🔥 Received {} from {} ({} of {}).",
                vote_display,
                sender_hash,
                threshold_decision.count,
                quorum_threshold
            );
            return None;
        };

        // We only generate the next qc once when we have a quorum of votes. Any votes received after this
        // are not included in the QC.
        if threshold_decision.count < quorum_threshold {
            info!(
                target: LOG_TARGET,
                "🔥 Received {} from {} ({} of {}).",
                vote_display,
                sender_hash,
                threshold_decision.count,
                quorum_threshold
            );
            return None;
        }

        info!(
            target: LOG_TARGET,
            "🔥 Received {} from {} ({} of {}). QUORUM!",
            vote_display,
            sender_hash,
            threshold_decision.count,
            quorum_threshold
        );

        let votes = access_mut.take_votes_with_decision_for_key(epoch, height, &key, quorum_decision)?;

        Some((votes, quorum_decision))
    }
}

/// Collection of votes indexed by sender leaf hash
type SenderVotesCollection<V> = HashMap<FixedHash, V>;

#[derive(Debug, Default)]
struct VoteStoreInner<V: Vote> {
    /// Maps (Epoch,Height) -> map (key -> map (sender leaf hash -> vote))
    /// The key is some type that uniquely keys the vote e.g BlockId or (Epoch,Height)
    store: BTreeMap<(Epoch, NodeHeight), HashMap<V::Key, SenderVotesCollection<V>>>,
}

impl<V: Vote + Display> VoteStoreInner<V> {
    const VOTE_BYTE_SIZE: usize = size_of::<V>() + size_of::<FixedHash>();
    const VOTE_KEY_SIZE: usize = size_of::<V::Key>();

    pub fn new() -> Self {
        Self { store: BTreeMap::new() }
    }

    fn clear_votes_before(&mut self, epoch: Epoch, height: NodeHeight) {
        self.store = self.store.split_off(&(epoch, height.saturating_sub(NodeHeight(1))));
        self.log_buffer_size();
    }

    /// Save a vote to the store. Returns false if a previous vote for the block by the sender was already present,
    /// otherwise true. Even if the vote is different, it is not considered a duplicate/invalid and does not
    /// overwrite the previous vote.
    pub fn save_vote(&mut self, sender_hash: FixedHash, vote: V) -> bool {
        let epoch_height = (vote.epoch(), vote.height());
        let view_votes_mut = self.store.entry(epoch_height).or_default();
        let votes_mut = view_votes_mut.entry(vote.key()).or_default();
        if votes_mut.contains_key(&sender_hash) {
            warn!(
                target: LOG_TARGET,
                "❓️ Received duplicate vote for {} from {} (sender hash). This could be malicious because a validator should only vote once for the same block.",
                vote,
                sender_hash
            );
            // We already have a vote for this block from this sender
            return false;
        }

        votes_mut.insert(sender_hash, vote);
        true
    }

    pub fn take_votes_with_decision_for_key(
        &mut self,
        epoch: Epoch,
        height: NodeHeight,
        key: &V::Key,
        decision: QuorumDecision,
    ) -> Option<Vec<V>> {
        let epoch_height = (epoch, height);
        let mut keyed_votes = self.store.remove(&epoch_height)?;
        // Discard any other votes for this epoch/height that are not for the key
        let votes = keyed_votes.remove(key)?;
        if keyed_votes.is_empty() {
            // Remove the empty map
            self.store.remove(&epoch_height);
        }
        Some(votes.into_values().filter(|vote| vote.decision() == decision).collect())
    }

    fn votes_for_key_iter(&self, epoch: Epoch, height: NodeHeight, key: &V::Key) -> Option<impl Iterator<Item = &V>> {
        let epoch_height = (epoch, height);
        let epoch_height_votes = self.store.get(&epoch_height)?;
        let votes = epoch_height_votes.get(key)?;
        Some(votes.values())
    }

    pub fn calculate_threshold_decision(
        &self,
        epoch: Epoch,
        height: NodeHeight,
        key: &V::Key,
        quorum_threshold: usize,
    ) -> ThresholdDecision {
        let Some(votes_iter) = self.votes_for_key_iter(epoch, height, key) else {
            // Soft invariant protection - technically, this should not happen but the correct ThresholdDecision is
            // returned regardless i.e. 0 votes
            error!(
                target: LOG_TARGET,
                "INVARIANT: calculate_threshold_decision: no votes for vote ({}/{}). Votes should have been collected before calling this function",
                epoch, height,
            );

            return ThresholdDecision {
                count: 0,
                decision: None,
            };
        };
        let mut count_accept = 0;
        let mut count_reject = 0;
        for vote in votes_iter {
            match vote.decision() {
                QuorumDecision::Accept => count_accept += 1,
                QuorumDecision::Reject => count_reject += 1,
            }
        }

        if count_accept >= quorum_threshold {
            return ThresholdDecision {
                count: count_accept,
                decision: Some(QuorumDecision::Accept),
            };
        }
        if count_reject >= quorum_threshold {
            return ThresholdDecision {
                count: count_reject,
                decision: Some(QuorumDecision::Reject),
            };
        }

        ThresholdDecision {
            count: count_accept + count_reject,
            decision: None,
        }
    }

    fn log_buffer_size(&self) {
        debug!(
            target: LOG_TARGET,
            "Vote store size: used: {:.2?}KiB ({} entries)",
            self.store.values() .map(|v|
                v.len() * Self::VOTE_KEY_SIZE +
                v.values().map(|v| v.len()).sum::<usize>() * Self::VOTE_BYTE_SIZE
            ).sum::<usize>() as f32 / 1024f32,
            self.store.len(),
        );
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ThresholdDecision {
    pub decision: Option<QuorumDecision>,
    pub count: usize,
}
