//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, ShardGroup};

/// Breadcrumb appended by the offline `tari_validator_rollback` tool after each successful
/// rollback. Append-only. Records what was rolled back and when — not the full audit
/// detail, which lives in the operator-supplied audit file.
///
/// `audit_file_basename` is the filename (no path) of the audit file the tool emitted
/// at the same moment; operators can use it to cross-reference the archived audit with
/// the DB's record that a rollback happened.
///
/// This type lives in `tari_ootle_storage` (rather than the rollback tool) only because
/// it is the value type of `RollbackHistoryCf` registered in the rocksdb schema. Reads
/// and writes both go through the rollback tool's `storage` module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackHistoryEntry {
    pub target_epoch: Epoch,
    pub shard_group: ShardGroup,
    pub applied_at_unix_secs: u64,
    pub tool_version: String,
    pub audit_file_basename: String,
}
