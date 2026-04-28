//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Storage models exclusive to the offline rollback tool.
//!
//! These were previously housed in `tari_ootle_storage::consensus_models` but are
//! never read or written by consensus itself, so they live alongside the
//! rocksdb-direct storage functions in [`super::read`] / [`super::write`].
//!
//! `RollbackHistoryEntry` is the one type that stays in `tari_ootle_storage` — it is
//! the value type of `RollbackHistoryCf`, which is registered in the rocksdb schema.

use serde::{Deserialize, Serialize};
use tari_consensus_types::BlockId;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{Epoch, shard::Shard};
use tari_ootle_transaction::TransactionId;
use tari_state_tree::Version;

/// One row per `StateTransitionRecordData` entry that would be reverted. Ordering is
/// the *reverse application order* — i.e. the same order in which the mutating rewind
/// inverts them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubstateRewindPlanRow {
    pub substate_id: SubstateId,
    pub shard: Shard,
    pub state_version: Version,
    pub transition: RewindTransitionKind,
    pub epoch: Epoch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RewindTransitionKind {
    /// Original transition was `Up` (substate created at this version). The rewind
    /// deletes the substate record.
    UpReverted,
    /// Original transition was `Down` (substate destroyed at this version). The rewind
    /// clears the `destroyed` field, restoring the substate to the alive state it
    /// held before this transition.
    DownReverted,
}

/// One row per block that would be deleted by [`super::write::rollback_delete_after_epoch`].
/// Carries the tx ids the block *finalised*, which is what indexers need to know about
/// to mark the corresponding user-visible transaction results as no-longer-committed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlocksAfterEpochRow {
    pub block_id: BlockId,
    pub epoch: Epoch,
    pub finalising_transaction_ids: Vec<TransactionId>,
}

/// Outcome of [`super::write::rollback_delete_after_epoch`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RollbackDeleteStats {
    pub blocks_deleted: usize,
    pub certificates_deleted: usize,
    pub checkpoints_deleted: usize,
    pub foreign_proposals_deleted: usize,
    pub validator_stats_deleted: usize,
    pub bookkeeping_cleared: bool,
}

/// Outcome of [`super::write::state_tree_truncate_to_version`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateTreeTruncateStats {
    pub nodes_deleted: usize,
    pub stale_records_deleted: usize,
}

/// Outcome of [`super::write::substates_rewind_to_state_version`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubstateRewindStats {
    pub transitions_processed: usize,
    pub substates_created_deleted: usize,
    pub substates_destroyed_restored: usize,
    pub heads_updated: usize,
}
