//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod bounded_spawn;
pub mod consensus_constants;
pub mod hotstuff;
pub mod messages;
mod tracing;
pub mod traits;
mod validations;

// The QC signature check is used by recovery probes outside the consensus crate; the others are exercised
// directly by the consensus test suite.
pub use validations::{
    check_block_commits_to_timeout_certificate,
    check_justify_precedes_block,
    check_justify_reaches_timeout_certificate,
    check_quorum_certificate_signatures,
    check_timeout_certificate_precedes_block,
    is_authenticated_commit_proof_stored,
    resolve_foreign_committee,
};
