//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

/// Outcome of `rollback_delete_after_epoch`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RollbackDeleteStats {
    /// Blocks deleted (across all epochs > target).
    pub blocks_deleted: usize,
    /// Proposal + timeout certificates deleted.
    pub certificates_deleted: usize,
    /// Epoch checkpoints deleted.
    pub checkpoints_deleted: usize,
    /// Foreign proposals deleted.
    pub foreign_proposals_deleted: usize,
    /// Validator epoch stats deleted.
    pub validator_stats_deleted: usize,
    /// Bookkeeping singletons cleared.
    pub bookkeeping_cleared: bool,
}
