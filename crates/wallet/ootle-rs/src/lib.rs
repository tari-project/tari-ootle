// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

pub mod builtin_templates;
pub mod key_provider;
pub mod provider;
pub mod signer;
pub mod transaction;
pub mod wallet;

#[macro_use]
pub mod macros;

pub mod keys;
pub mod stealth;
mod types;

// Re-export the address macro from the ootle_address crate
pub use tari_ootle_address::address;
pub use types::*;
