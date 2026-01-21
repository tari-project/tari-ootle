//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use tari_common_types::seeds::cipher_seed::CipherSeed;
use tari_ootle_wallet_crypto::encryption::CipherError;

use crate::{
    apis::password_manager::PasswordManagerApiError,
    cipher_seed::WalletCipherSeed,
    key_managers::WithCipherSeed,
    storage::WalletStorageError,
};

#[derive(Clone)]
pub struct LocalKeyStore {
    cipher_seed: WalletCipherSeed,
}

impl LocalKeyStore {
    pub fn new(cipher_seed: WalletCipherSeed) -> Self {
        Self { cipher_seed }
    }

    pub fn set_cipher_seed(&mut self, cipher_seed: CipherSeed) -> &mut Self {
        self.cipher_seed = WalletCipherSeed::CipherSeed(Arc::new(cipher_seed));
        self
    }

    pub fn cipher_seed(&self) -> Option<&CipherSeed> {
        self.cipher_seed.cipher_seed()
    }

    pub fn into_cipher_seed(self) -> WalletCipherSeed {
        self.cipher_seed
    }
}
impl WithCipherSeed for LocalKeyStore {
    type Error = LocalKeyStoreError;

    fn get_cipher_seed(&self) -> Result<&CipherSeed, Self::Error> {
        self.cipher_seed().ok_or(LocalKeyStoreError::NoCipherSeed)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum LocalKeyStoreError {
    #[error("Password manager error: {0}")]
    PasswordManager(#[from] PasswordManagerApiError),
    #[error("Wallet storage error: {0}")]
    WalletStorage(#[from] WalletStorageError),
    #[error("Cipher error: {0}")]
    Cipher(#[from] CipherError),
    #[error("Cannot derive keys because no cipher seed was provided")]
    NoCipherSeed,
}
