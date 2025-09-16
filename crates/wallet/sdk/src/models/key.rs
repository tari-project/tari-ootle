//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey};
use tari_transaction_components::key_manager::tari_key_manager::DerivedKey;

#[derive(Clone)]
pub struct WalletKey {
    pub branch: String,
    pub key_pair: KeyPair,
    pub is_active: bool,
}

impl WalletKey {
    pub fn key_index(&self) -> u64 {
        self.key_pair.secret_key.key_index
    }

    pub fn public_key(&self) -> &RistrettoPublicKey {
        &self.key_pair.public_key
    }
}

#[derive(Clone)]
pub struct KeyPair {
    pub public_key: RistrettoPublicKey,
    pub secret_key: DerivedKey,
}

impl KeyPair {
    pub fn key_index(&self) -> u64 {
        self.secret_key.key_index
    }

    pub fn public_key(&self) -> &RistrettoPublicKey {
        &self.public_key
    }

    pub fn secret_key(&self) -> &RistrettoSecretKey {
        &self.secret_key.key
    }
}
