//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

/// Outcome of `state_tree_truncate_to_version`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateTreeTruncateStats {
    /// Number of tree-node entries deleted (JMT internal nodes + leaves at versions > target).
    pub nodes_deleted: usize,
    /// Number of stale-node records deleted (versions > target).
    pub stale_records_deleted: usize,
}
