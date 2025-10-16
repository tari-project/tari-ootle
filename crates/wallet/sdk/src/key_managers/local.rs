//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use digest::crypto_common::rand_core::OsRng;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSchnorr},
};
use tari_ootle_common_types::optional::IsNotFoundError;

use crate::{
    apis::password_manager::PasswordManagerApiError,
    key_managers::{backend::WalletKeyStore, KeyManagerBackend, SignatureOutput},
    models::{ImportedKeyId, KeyId},
    storage::WalletStorageError,
};

#[derive(Debug, Clone)]
pub struct LocalKeyManager<TKeyStore> {
    key_store: TKeyStore,
}

impl<TKeyStore: WalletKeyStore<ImportedKeyId>> LocalKeyManager<TKeyStore> {
    pub fn new(key_store: TKeyStore) -> Self {
        Self { key_store }
    }
}

impl<M, TKeyStore> KeyManagerBackend<M> for LocalKeyManager<TKeyStore>
where
    M: AsRef<[u8]>,
    TKeyStore: WalletKeyStore<ImportedKeyId>,
{
    type Error = LocalKeyManagerError<TKeyStore::Error>;

    fn try_sign(&mut self, branch: &str, key_id: KeyId, message: M) -> Result<SignatureOutput, Self::Error> {
        let secret = match key_id {
            KeyId::Derived { index } => self
                .key_store
                .derive_secret(branch, index)
                .map_err(LocalKeyManagerError::KeyStoreError)?,
            KeyId::Imported { local_key_id } => self
                .key_store
                .get_imported_secret(local_key_id)
                .map_err(LocalKeyManagerError::KeyStoreError)?,
        };
        let signature = RistrettoSchnorr::sign(&secret, message, &mut OsRng)
            .expect("RistrettoSchnorr::sign is infallible as it internally hashes the message into canonical form");
        let public_key = RistrettoPublicKey::from_secret_key(&secret);
        Ok(SignatureOutput { signature, public_key })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LocalKeyManagerError<TKeyStoreErr> {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Password manager error: {0}")]
    PasswordManagerApiError(#[from] PasswordManagerApiError),
    #[error("Key manager is in read only mode")]
    ReadOnlyMode,
    #[error("Cipher error: {0}")]
    KeyStoreError(TKeyStoreErr),
}

impl<TKeyStoreErr> IsNotFoundError for LocalKeyManagerError<TKeyStoreErr> {
    fn is_not_found_error(&self) -> bool {
        matches!(self, LocalKeyManagerError::StoreError(e) if e.is_not_found_error())
    }
}
