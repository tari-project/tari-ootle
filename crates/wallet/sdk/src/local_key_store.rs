//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::{ristretto::RistrettoSecretKey, tari_utilities::ByteArray};
use tari_ootle_wallet_crypto::encryption::{decrypt_with_password, CipherError};

use crate::{
    apis::password_manager::{PasswordManagerApi, PasswordManagerApiError},
    key_managers::WalletKeyStore,
    models::ImportedKeyId,
    storage::{WalletStorageError, WalletStore, WalletStoreReader},
};

#[derive(Clone)]
pub struct LocalKeyStore<'a, TStore> {
    password_manager_api: PasswordManagerApi<'a, TStore>,
    wallet_store: &'a TStore,
}

impl<'a, TStore> LocalKeyStore<'a, TStore> {
    pub fn new(password_manager_api: PasswordManagerApi<'a, TStore>, wallet_store: &'a TStore) -> Self {
        Self {
            password_manager_api,
            wallet_store,
        }
    }
}

impl<TStore: WalletStore> WalletKeyStore<ImportedKeyId> for LocalKeyStore<'_, TStore> {
    type Error = LocalKeyStoreError;

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
}

#[derive(thiserror::Error, Debug)]
pub enum LocalKeyStoreError {
    #[error("Password manager error: {0}")]
    PasswordManager(#[from] PasswordManagerApiError),
    #[error("Wallet storage error: {0}")]
    WalletStorage(#[from] WalletStorageError),
    #[error("Cipher error: {0}")]
    Cipher(#[from] CipherError),
}
