//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use tari_common_types::seeds::cipher_seed::CipherSeed;
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_ootle_wallet_crypto::encryption::CipherError;

use crate::{
    apis::{key_manager::WalletKeyManager, password_manager::PasswordManagerApiError},
    cipher_seed::WalletCipherSeed,
    key_managers::WalletKeyStore,
    models::DerivedKeyIndex,
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

    fn get_cipher_seed(&self) -> Result<&CipherSeed, LocalKeyStoreError> {
        self.cipher_seed().ok_or(LocalKeyStoreError::NoCipherSeed)
    }
}

impl WalletKeyStore for LocalKeyStore {
    type Error = LocalKeyStoreError;

    fn derive_secret(&self, branch: &str, key_index: DerivedKeyIndex) -> Result<RistrettoSecretKey, Self::Error> {
        let cipher_seed = self.get_cipher_seed()?;
        let km = WalletKeyManager::from(cipher_seed.clone(), branch.to_string(), 0);
        let secret = km
            .derive_key(key_index)
            .expect("Key derivation bug: derive key internally creates a canonical key and must not fail");
        Ok(secret.key)
    }

    fn key_birthday(&self) -> Result<Option<u16>, Self::Error> {
        let seed = self.get_cipher_seed()?;
        Ok(Some(seed.birthday()))
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
