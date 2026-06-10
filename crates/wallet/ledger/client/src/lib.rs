//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Host-side client for the Tari Ootle Ledger app.
//!
//! [`LedgerClient`] wraps any [`ledger_transport::Exchange`] transport and exposes the app's
//! instruction set: app name/version queries, on-device key derivation, and streamed transaction
//! signing (authorization and seal signatures, with optional stealth key derivation). The wire
//! format — instructions, status words, and request/response bodies — comes from
//! `ootle_ledger_common`, which the device app also depends on, so both sides share a single
//! protocol definition.
//!
//! Transports are feature-gated:
//! - `hid-transport` — `LedgerHidClient`, native USB HID for physical devices.
//! - `speculos-transport` — `SpeculosTransport`, which drives the [Speculos](https://github.com/LedgerHQ/speculos)
//!   emulator's REST API; used by the integration tests.

mod client;
mod error;
#[cfg(feature = "hid-transport")]
mod hid;

pub use client::*;
pub use error::*;
#[cfg(feature = "hid-transport")]
pub use hid::*;
// Re-exported so downstream signers (e.g. ootle-rs) can name the transport bound.
pub use ledger_transport::{self, Exchange};

mod decode;
#[cfg(feature = "speculos-transport")]
pub mod speculos_transport;
