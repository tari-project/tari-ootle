//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Ergonomic builders for interacting with Ootle's built-in templates.
//!
//! - [`account::IAccount`] — public transfers, fee payment, and template publishing.
//! - [`faucet::IFaucet`] — claim free testnet tokens from the faucet.
//! - [`component`] — generic component and template invocation, including the
//!   [`ootle_template!`](crate::ootle_template) macro for type-safe method calls.

pub mod account;
pub mod component;
pub mod faucet;
mod traits;

pub use traits::*;
