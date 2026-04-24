//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Library surface for the offline rollback tool.
//!
//! The companion binary in `src/main.rs` is a thin clap-based wrapper over the
//! functions re-exported here. Embedding callers (integration tests, runbook
//! automation) should prefer [`apply::run_with_options`] to driving the CLI.

pub mod apply;
pub mod audit;
pub mod audit_writer;
pub mod convert;
pub mod inspect;
