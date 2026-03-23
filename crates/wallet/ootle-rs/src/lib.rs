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

mod helpers;
pub mod keys;
pub mod stealth;
mod types;

// Re-export the address macro from the ootle_address crate
pub use helpers::*;
pub use tari_ootle_address::address;
pub use tari_ootle_common_types::{Network, displayable};
pub use tari_ootle_wallet_crypto as crypto;
pub use tari_template_lib_types as template_types;
pub use types::*;
