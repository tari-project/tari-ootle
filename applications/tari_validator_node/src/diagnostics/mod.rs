//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The validator's local diagnostic event log: a bounded, queryable record of the abnormal moments
//! a node goes through, so an operator can answer "what went wrong, and when" without log files.

mod handle;
mod hooks;
mod panic;
mod writer;

pub use handle::DiagnosticsHandle;
pub use hooks::DiagnosticHooks;
pub use panic::{install_panic_recorder, record_panic};
pub use writer::spawn;
