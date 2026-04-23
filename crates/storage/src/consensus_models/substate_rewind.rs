//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

/// Outcome of `substates_rewind_to_state_version`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubstateRewindStats {
    /// Number of `StateTransitionCf` records inverted and deleted.
    pub transitions_processed: usize,
    /// Number of `SubstateRecord`s deleted (inverses of `Up` transitions).
    pub substates_created_deleted: usize,
    /// Number of `SubstateRecord`s whose `destroyed` was cleared (inverses of `Down` transitions).
    pub substates_destroyed_restored: usize,
    /// Number of `HeadIndex` entries rebuilt (either updated or deleted).
    pub heads_updated: usize,
}
