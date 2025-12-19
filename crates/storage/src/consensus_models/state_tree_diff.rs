//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, fmt::Display, ops::Deref};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tari_consensus_types::BlockId;
use tari_ootle_common_types::shard::Shard;
use tari_state_tree::{StateHashTreeDiff, Version};

use crate::{StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingShardStateTreeDiff {
    pub version: Version,
    pub diff: StateHashTreeDiff,
}

impl PendingShardStateTreeDiff {
    pub fn new(version: Version, diff: StateHashTreeDiff) -> Self {
        Self { version, diff }
    }

    pub fn load(version: Version, diff: StateHashTreeDiff) -> Self {
        Self { version, diff }
    }
}

impl PendingShardStateTreeDiff {
    /// Returns all pending state tree diffs from the last committed block (exclusive) to the given block (inclusive).
    pub fn get_all_up_to_commit_block<TTx>(
        tx: &TTx,
        block_id: &BlockId,
    ) -> Result<HashMap<Shard, Vec<Self>>, StorageError>
    where
        TTx: StateStoreReadTransaction,
    {
        tx.pending_state_tree_diffs_get_all_up_to_commit_block(block_id)
    }

    pub fn remove_by_block<TTx>(tx: &mut TTx, block_id: &BlockId) -> Result<IndexMap<Shard, Vec<Self>>, StorageError>
    where
        TTx: Deref + StateStoreWriteTransaction,
        TTx::Target: StateStoreReadTransaction,
    {
        tx.pending_state_tree_diffs_remove_and_return_by_block(block_id)
    }

    pub fn create<TTx>(
        tx: &mut TTx,
        block_id: BlockId,
        shard: Shard,
        diff: &PendingShardStateTreeDiff,
    ) -> Result<(), StorageError>
    where
        TTx: Deref + StateStoreWriteTransaction,
        TTx::Target: StateStoreReadTransaction,
    {
        tx.pending_state_tree_diffs_insert(block_id, shard, diff)
    }
}

impl Display for PendingShardStateTreeDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PendingShardStateTreeDiff(v{}, {} new node(s), {} value(s), {} stale node(s))",
            self.version,
            self.diff.new_nodes.nodes.len(),
            self.diff.new_nodes.values.len(),
            self.diff.stale_tree_nodes.len()
        )
    }
}
