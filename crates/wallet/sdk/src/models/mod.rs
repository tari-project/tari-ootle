//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod account;
mod authored_template;
mod confidential_output;
mod config;
mod event;
mod key;
mod lock_guard;
mod non_fungible_tokens;
mod resource;
mod stealth_output;
mod substate;
mod utxo_update;
mod vault;
mod wallet_transaction;
mod webauthn_registration;

pub use account::*;
pub use authored_template::*;
pub use confidential_output::*;
pub use config::Config;
pub use event::*;
pub use key::*;
pub use lock_guard::*;
pub use non_fungible_tokens::*;
pub use resource::*;
pub use stealth_output::*;
pub use substate::*;
pub use utxo_update::*;
pub use vault::*;
pub use wallet_transaction::*;
pub use webauthn_registration::*;

pub type WalletLockId = i32;
