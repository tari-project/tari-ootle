//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Read-only "dry run" models for rollback impact analysis.
//!
//! These are consumed by the offline `tari_validator_rollback` tool to build an audit
//! file of what a rollback would (or did) touch, without mutating storage. The two
//! collect-* trait methods on `StateStoreReadTransaction` walk the same CFs as
//! `substates_rewind_to_state_version` / `rollback_delete_after_epoch`; the mirror is
//! load-bearing — dry-run and apply must observe identical state, anchored by a unit test.

use serde::{Deserialize, Serialize};
use tari_consensus_types::BlockId;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{Epoch, shard::Shard};
use tari_ootle_transaction::TransactionId;
use tari_state_tree::Version;

/// One row per `StateTransitionRecordData` entry that would be reverted. Ordering is
/// the *reverse application order* — i.e. same order in which the mutating rewind
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
    /// clears the `destroyed` field, restoring the substate to the "alive" state it
    /// held before this transition.
    DownReverted,
}

/// One row per block that would be deleted by `rollback_delete_after_epoch`. Contains
/// the tx ids the block *finalised*, which is what indexers need to know about to
/// mark the corresponding user-visible transaction results as no-longer-committed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlocksAfterEpochRow {
    pub block_id: BlockId,
    pub epoch: Epoch,
    pub finalising_transaction_ids: Vec<TransactionId>,
}
