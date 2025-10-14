//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use blake2::Blake2b;
use digest::{consts::U64, crypto_common::rand_core::OsRng};
use tari_common_types::seeds::cipher_seed::CipherSeed;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSchnorr},
};
use tari_ootle_common_types::optional::IsNotFoundError;
use tari_transaction_components::key_manager::tari_key_manager::TariKeyManager;

use crate::{
    apis::password_manager::PasswordManagerApiError,
    key_managers::{backend::WalletKeyStore, KeyManagerBackend, SignatureOutput},
    models::{ImportedKeyId, KeyId},
    storage::WalletStorageError,
};

type WalletKeyManager = TariKeyManager<Blake2b<U64>>;

#[derive(Debug, Clone)]
pub struct LocalKeyManager<'a, TKeyStore> {
    cipher_seed: &'a CipherSeed,
    key_store: TKeyStore,
}

impl<'a, TKeyStore: WalletKeyStore<ImportedKeyId>> LocalKeyManager<'a, TKeyStore> {
    pub fn new(cipher_seed: &'a CipherSeed, key_store: TKeyStore) -> Self {
        Self { cipher_seed, key_store }
    }

    /// WARNING: dont use next_key on the key manager because this will always return the same key
    fn get_key_manager(&mut self, branch: &str) -> WalletKeyManager {
        WalletKeyManager::from(self.cipher_seed.clone(), branch.to_string(), 0)
    }
}

impl<M, TKeyStore> KeyManagerBackend<M> for LocalKeyManager<'_, TKeyStore>
where
    M: AsRef<[u8]>,
    TKeyStore: WalletKeyStore<ImportedKeyId>,
{
    type Error = LocalKeyManagerError<TKeyStore::Error>;

    fn try_sign(&mut self, branch: &str, key_id: KeyId, message: M) -> Result<SignatureOutput, Self::Error> {
        let secret = match key_id {
            KeyId::Derived { index } => {
                let km = self.get_key_manager(branch);
                let key = km
                    .derive_key(index)
                    .expect("BUG: Key derivation is infallible because it internally hashes to a canonical form.");
                key.key
            },
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
