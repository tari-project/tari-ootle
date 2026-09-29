//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use tari_consensus_types::BlockId;
use tari_ootle_common_types::NodeHeight;

/// The uncommitted blocks from a leaf block back to the commit block, read once so that many branch-scoped reads at
/// the same leaf can share it.
///
/// A `PendingChain` is a snapshot of the pending-chain index as it was when it was read. Inserting or committing a
/// block changes that index, so a chain must be read after any such write in the same transaction and not used across
/// one. Holding it next to a shared borrow of the transaction that produced it, which rules out writes for as long as
/// the chain lives, is the simplest way to keep it current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChain {
    leaf: BlockId,
    /// The leaf and its uncommitted ancestors, ordered from the leaf down.
    blocks: Vec<BlockId>,
    pending: HashSet<BlockId>,
    /// `pending` plus the blocks the walk reached beneath it: the commit block, or the zero block.
    ancestry: HashSet<BlockId>,
    commit_height: Option<NodeHeight>,
}

impl PendingChain {
    /// Builds a chain from a walk of the pending-chain index.
    ///
    /// `walk` starts at `leaf` and follows parent links until the index has no entry or the zero block is reached,
    /// including that final parent. It is empty when `leaf` is not in the index. The chain's blocks are the walk up to
    /// the commit block or the zero block; the leaf is always one of them when the walk is non-empty. `commit` is the
    /// commit block's id and height, absent before any block has committed.
    pub fn from_walk(leaf: BlockId, walk: Vec<BlockId>, commit: Option<(BlockId, NodeHeight)>) -> Self {
        let ancestry = walk.iter().copied().collect();
        let commit_block_id = commit.map(|(id, _)| id);
        let pending_len = walk
            .iter()
            .skip(1)
            .position(|id| id.is_zero() || Some(*id) == commit_block_id)
            .map_or(walk.len(), |pos| pos + 1);
        let mut blocks = walk;
        blocks.truncate(pending_len);
        let pending = blocks.iter().copied().collect();
        Self {
            leaf,
            blocks,
            pending,
            ancestry,
            commit_height: commit.map(|(_, height)| height),
        }
    }

    pub fn leaf(&self) -> &BlockId {
        &self.leaf
    }

    /// The leaf and its uncommitted ancestors, ordered from the leaf down. Empty when the leaf is not pending.
    pub fn blocks(&self) -> &[BlockId] {
        &self.blocks
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Whether `block_id` is one of [`Self::blocks`].
    pub fn contains_pending(&self, block_id: &BlockId) -> bool {
        self.pending.contains(block_id)
    }

    /// Whether `block_id` is one of [`Self::blocks`] or the block the chain rests on.
    pub fn contains_with_base(&self, block_id: &BlockId) -> bool {
        self.ancestry.contains(block_id)
    }

    /// [`Self::blocks`] and the block the chain rests on.
    pub fn ancestry(&self) -> &HashSet<BlockId> {
        &self.ancestry
    }

    /// The height of the commit block when the chain was read, or `None` if no block had committed.
    pub fn commit_height(&self) -> Option<NodeHeight> {
        self.commit_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> BlockId {
        BlockId::new([n; 32])
    }

    #[test]
    fn it_stops_the_pending_blocks_at_the_commit_block() {
        let chain = PendingChain::from_walk(id(3), vec![id(3), id(2), id(1)], Some((id(1), NodeHeight(1))));
        assert_eq!(chain.blocks(), &[id(3), id(2)]);
        assert!(chain.contains_pending(&id(2)));
        assert!(!chain.contains_pending(&id(1)));
        assert!(chain.contains_with_base(&id(1)));
    }

    #[test]
    fn it_stops_the_pending_blocks_at_the_zero_block() {
        let chain = PendingChain::from_walk(id(2), vec![id(2), id(1), BlockId::zero()], None);
        assert_eq!(chain.blocks(), &[id(2), id(1)]);
        assert!(chain.contains_with_base(&BlockId::zero()));
        assert!(!chain.contains_pending(&BlockId::zero()));
    }

    #[test]
    fn it_keeps_a_leaf_that_is_the_commit_block() {
        let chain = PendingChain::from_walk(id(1), vec![id(1), id(5)], Some((id(1), NodeHeight(1))));
        assert_eq!(chain.blocks(), &[id(1), id(5)]);
    }

    #[test]
    fn it_is_empty_for_a_leaf_outside_the_index() {
        let chain = PendingChain::from_walk(id(1), vec![], Some((id(1), NodeHeight(1))));
        assert!(chain.is_empty());
        assert!(!chain.contains_with_base(&id(1)));
    }
}
