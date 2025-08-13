//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::RistrettoPublicKey;
use tari_key_manager::key_manager::DerivedKey;
use tari_template_lib::prelude::RistrettoPublicKeyBytes;

#[derive(Debug, Clone)]
pub struct WalletKey {
    pub branch: String,
    pub public_key: RistrettoPublicKeyBytes,
    pub secret_key: DerivedKey<RistrettoPublicKey>,
    pub is_active: bool,
}

impl WalletKey {
    pub fn key_index(&self) -> u64 {
        self.secret_key.key_index
    }
}
