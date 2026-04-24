//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, ShardGroup};

use crate::{StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};

/// Breadcrumb written by the offline `tari_validator_rollback` tool after each successful
/// rollback. Append-only. Records what was rolled back and when — not the full audit
/// detail, which lives in the operator-supplied audit file.
///
/// The `audit_file_basename` is the filename (no path) of the audit file the tool
/// emitted at the same moment; operators can use it to cross-reference the archived
/// audit with the DB's record that a rollback happened.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackHistoryEntry {
    pub target_epoch: Epoch,
    pub shard_group: ShardGroup,
    pub applied_at_unix_secs: u64,
    pub tool_version: String,
    pub audit_file_basename: String,
}

impl RollbackHistoryEntry {
    pub fn insert<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.rollback_history_insert(self)
    }

    pub fn list<TTx: StateStoreReadTransaction>(tx: &TTx) -> Result<Vec<RollbackHistoryEntry>, StorageError> {
        tx.rollback_history_list()
    }
}
