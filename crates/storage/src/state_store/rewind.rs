//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

/// Outcome of [`crate::StateStoreWriteTransaction::state_tree_truncate_to_version`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateTreeTruncateStats {
    pub nodes_deleted: usize,
    pub stale_records_deleted: usize,
}

/// Outcome of [`crate::StateStoreWriteTransaction::substates_rewind_to_state_version`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubstateRewindStats {
    pub transitions_processed: usize,
    pub substates_created_deleted: usize,
    pub substates_destroyed_restored: usize,
    pub heads_updated: usize,
}
