//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::seeds::cipher_seed::CipherSeed;
use tari_crypto::{ristretto::RistrettoSecretKey, tari_utilities::ByteArray};
use tari_ootle_wallet_crypto::encryption::{decrypt_with_password, CipherError};

use crate::{
    apis::{
        key_manager::WalletKeyManager,
        password_manager::{PasswordManagerApi, PasswordManagerApiError},
    },
    cipher_seed::WalletCipherSeed,
    key_managers::WalletKeyStore,
    models::{DerivedKeyIndex, ImportedKeyId},
    storage::{WalletStorageError, WalletStore, WalletStoreReader},
};

#[derive(Clone)]
pub struct LocalKeyStore<'a, TStore> {
    password_manager_api: PasswordManagerApi<'a, TStore>,
    cipher_seed: &'a WalletCipherSeed,
    wallet_store: &'a TStore,
}

impl<'a, TStore> LocalKeyStore<'a, TStore> {
    pub fn new(
        cipher_seed: &'a WalletCipherSeed,
        password_manager_api: PasswordManagerApi<'a, TStore>,
        wallet_store: &'a TStore,
    ) -> Self {
        Self {
            cipher_seed,
            password_manager_api,
            wallet_store,
        }
    }

    fn get_cipher_seed(&self) -> Result<&CipherSeed, LocalKeyStoreError> {
        self.cipher_seed.cipher_seed().ok_or(LocalKeyStoreError::NoCipherSeed)
    }
}

impl<TStore: WalletStore> WalletKeyStore<ImportedKeyId> for LocalKeyStore<'_, TStore> {
    type Error = LocalKeyStoreError;

    fn derive_secret(&self, branch: &str, key_index: DerivedKeyIndex) -> Result<RistrettoSecretKey, Self::Error> {
        let cipher_seed = self.get_cipher_seed()?;
        let km = WalletKeyManager::from(cipher_seed.clone(), branch.to_string(), 0);
        let secret = km
            .derive_key(key_index)
            .expect("Key derivation bug: derive key internally creates a canonical key and must not fail");
        Ok(secret.key)
    }

    fn get_imported_secret(&self, key: ImportedKeyId) -> Result<RistrettoSecretKey, Self::Error> {
        let password = self.password_manager_api.get_cipher_seed_password()?;
        let (_ty, encrypted) = self
            .wallet_store
            .with_read_tx(|tx| tx.key_manager_get_raw_imported_key(key))?;
        let decrypted = decrypt_with_password(&encrypted, password.reveal())?;
        let secret = RistrettoSecretKey::from_canonical_bytes(&decrypted).map_err(|e| {
            LocalKeyStoreError::WalletStorage(WalletStorageError::DecodingError {
                operation: "get_imported_secret",
                item: "imported secret key",
                details: format!("Imported key at id {key} is non-canonical {e}"),
            })
        })?;
        Ok(secret)
    }

    fn cipher_seed_birthday(&self) -> Result<Option<u16>, Self::Error> {
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
