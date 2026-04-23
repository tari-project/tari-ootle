//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Admin/break-glass operations for validator nodes.
//!
//! Everything in this module is authenticated by the configured governance public key and
//! is exposed only via the separate admin JSON-RPC listener (see `spawn_admin_json_rpc`).

mod rollback;

pub use rollback::{RollbackError, RollbackOutcome, apply_rollback_directive};
