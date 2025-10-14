// Copyright 2022 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

pub mod storage;

pub mod apis;
pub mod models;
mod sdk;

pub use sdk::{WalletSdk, WalletSdkConfig};
pub use tari_common_types::seeds::cipher_seed::CipherSeed;

pub mod cipher_seed;
pub mod network;

pub type WalletSecretKey = tari_transaction_components::key_manager::tari_key_manager::DerivedKey;

// Re-export commonly used types
pub use tari_common_types::seeds::seed_words::SeedWords;
pub use tari_ootle_address::*;
pub use tari_ootle_common_types::Network;
pub use tari_ootle_wallet_crypto as crypto;
pub use tari_template_lib::constants;
