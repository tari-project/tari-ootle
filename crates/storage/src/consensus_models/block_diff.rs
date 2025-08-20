//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Debug;

use tari_consensus_types::BlockId;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{ShardGroup, VersionedSubstateIdRef};

use crate::{
    consensus_models::substate_change::SubstateChange,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};

#[derive(Debug, Clone)]
pub struct BlockDiff {
    block_id: BlockId,
    pub changes: Vec<SubstateChange>,
}

impl BlockDiff {
    pub fn new(block_id: BlockId, changes: Vec<SubstateChange>) -> Self {
        Self { block_id, changes }
    }

    pub fn empty(block_id: BlockId) -> Self {
        Self::new(block_id, vec![])
    }

    pub fn len(&self) -> usize {
        self.changes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn into_filtered(self, shard_group: ShardGroup) -> Self {
        Self {
            block_id: self.block_id,
            changes: self
                .changes
                .into_iter()
                .filter(|change| shard_group.contains_or_global(&change.shard()))
                .collect(),
        }
    }

    pub fn block_id(&self) -> &BlockId {
        &self.block_id
    }

    pub fn changes(&self) -> &[SubstateChange] {
        &self.changes
    }

    pub fn into_changes(self) -> Vec<SubstateChange> {
        self.changes
    }
}

impl BlockDiff {
    pub fn insert<TTx: StateStoreWriteTransaction>(
        tx: &mut TTx,
        block_id: &BlockId,
        changes: &[SubstateChange],
    ) -> Result<(), StorageError> {
        tx.block_diffs_insert(block_id, changes)
    }

    pub fn remove<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.block_diffs_remove(&self.block_id)
    }

    pub fn get_last_change_for_substate<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        block_id: &BlockId,
        substate_id: &SubstateId,
    ) -> Result<SubstateChange, StorageError> {
        tx.block_diffs_get_last_change_for_substate(block_id, substate_id)
    }

    pub fn get_for_versioned_substate<'a, TTx: StateStoreReadTransaction, T: Into<VersionedSubstateIdRef<'a>>>(
        tx: &TTx,
        block_id: &BlockId,
        substate_id: T,
    ) -> Result<SubstateChange, StorageError> {
        tx.block_diffs_get_change_for_versioned_substate(block_id, substate_id)
    }
}
