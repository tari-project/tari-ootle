//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use blake2::Blake2b;
use digest::{consts::U64, crypto_common::rand_core::OsRng};
use tari_crypto::{
    keys::{PublicKey as _, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_ootle_address::RistrettoOotleAddress;
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    Network,
};
use tari_ootle_wallet_crypto::encryption::{decrypt_with_password, encrypt_with_password};
use tari_transaction_components::{key_manager, key_manager::tari_key_manager::TariKeyManager};

use crate::{
    apis::password_manager::{PasswordManagerApi, PasswordManagerApiError},
    cipher_seed::WalletCipherSeed,
    models::{
        DerivedKeyIndex,
        DerivedKeyPair,
        DerivedWalletKey,
        ImportedKeyId,
        ImportedWalletKey,
        KeyBranch,
        KeyId,
        KeyType,
        WalletKeyRecord,
        WalletOotleAddressWithKeyIds,
        WalletPublicKey,
        WalletSecretKey,
    },
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

pub type WalletKeyManager = TariKeyManager<Blake2b<U64>>;

#[derive(Clone)]
pub struct KeyManagerApi<'a, TStore> {
    network: Network,
    store: &'a TStore,
    cipher_seed: &'a WalletCipherSeed,
    password_manager: PasswordManagerApi<'a, TStore>,
}

impl<'a, TStore: WalletStore> KeyManagerApi<'a, TStore> {
    pub(crate) fn new(
        network: Network,
        store: &'a TStore,
        cipher_seed: &'a WalletCipherSeed,
        password_manager: PasswordManagerApi<'a, TStore>,
    ) -> Self {
        Self {
            network,
            store,
            cipher_seed,
            password_manager,
        }
    }

    pub fn get_all_derived_keys(&self, branch: KeyBranch) -> Result<Vec<WalletKeyRecord>, KeyManagerApiError> {
        let all_keys = self.store.with_read_tx(|tx| tx.key_manager_get_all(branch.as_str()))?;
        let mut keys = Vec::with_capacity(all_keys.len());
        let km = self.get_key_manager(branch.as_str())?;
        for (index, active) in all_keys {
            let key = km
                .derive_key(index)
                .map_err(key_manager::error::KeyManagerServiceError::from)?;
            let pk = RistrettoPublicKey::from_secret_key(&key.key);
            keys.push(WalletKeyRecord {
                key_id: KeyId::derived(index),
                public_key: pk,
                secret_key: key.key,
                is_active: active,
            });
        }
        Ok(keys)
    }

    pub fn get_imported_key(&self, id: ImportedKeyId) -> Result<ImportedWalletKey, KeyManagerApiError> {
        let password = self.password_manager.get_cipher_seed_password()?;
        self.store.with_read_tx(|tx| {
            let (key_type, encrypted_key) = tx.key_manager_get_raw_imported_key(id)?;
            let decrypted = decrypt_with_password(&encrypted_key, password.reveal()).map_err(|e| {
                KeyManagerApiError::StoreError(WalletStorageError::DecryptionError {
                    operation: "KeyManagerApi::get_imported_key",
                    details: format!("Failed to decrypt imported key: {}", e),
                })
            })?;
            Ok(ImportedWalletKey {
                key: RistrettoSecretKey::from_canonical_bytes(&decrypted).map_err(|_| {
                    KeyManagerApiError::StoreError(WalletStorageError::DecodingError {
                        operation: "KeyManagerApi::get_imported_key",
                        item: "imported_key",
                        details: "Failed to decode imported key".to_string(),
                    })
                })?,
                import_id: id,
                key_type,
            })
        })
    }

    pub fn import_key(
        &self,
        label: &str,
        secret_key: &RistrettoSecretKey,
        key_type: KeyType,
    ) -> Result<KeyId, KeyManagerApiError> {
        let password = self.password_manager.get_cipher_seed_password()?;
        let encrypted_key = encrypt_with_password(secret_key.as_bytes(), password.reveal()).map_err(|e| {
            KeyManagerApiError::StoreError(WalletStorageError::EncryptionError {
                operation: "KeyManagerApi::import_key",
                details: format!("Failed to encrypt imported key: {}", e),
            })
        })?;
        let id = self
            .store
            .with_write_tx(|tx| tx.key_manager_insert_imported_key(label, &encrypted_key, key_type))?;
        Ok(KeyId::imported(id))
    }

    pub fn get_account_owner_key(&self, key_id: KeyId) -> Result<WalletSecretKey, KeyManagerApiError> {
        self.get_key(KeyBranch::Account, key_id)
    }

    pub fn get_view_only_key(&self, key_id: KeyId) -> Result<WalletSecretKey, KeyManagerApiError> {
        self.get_key(KeyBranch::ViewOnlyKey, key_id)
    }

    pub(crate) fn get_key(&self, branch: KeyBranch, key_id: KeyId) -> Result<WalletSecretKey, KeyManagerApiError> {
        match key_id {
            KeyId::Imported { local_key_id } => {
                let imported_key = self.get_imported_key(local_key_id)?;
                Ok(imported_key.into())
            },
            KeyId::Derived { index } => {
                let derived_key = self.derive_key(branch, index)?;
                Ok(derived_key.into())
            },
        }
    }

    pub fn get_public_key(&self, branch: KeyBranch, key_id: KeyId) -> Result<WalletPublicKey, KeyManagerApiError> {
        match key_id {
            KeyId::Imported { local_key_id } => {
                // TODO: could be implemented without fetching the secret key, if we stored the public key in the DB
                let imported_key = self.get_imported_key(local_key_id)?;
                Ok(WalletPublicKey {
                    public_key: imported_key.to_public_key(),
                    key_id,
                })
            },
            KeyId::Derived { index } => {
                let derived_key = self.derive_key(branch, index)?;
                Ok(WalletPublicKey {
                    public_key: derived_key.to_public_key(),
                    key_id,
                })
            },
        }
    }

    pub fn get_elgamal_encrypted_view_key(
        &self,
        index: DerivedKeyIndex,
    ) -> Result<DerivedWalletKey, KeyManagerApiError> {
        self.derive_key(KeyBranch::ElgamalEncryptionViewKey, index)
    }

    pub(crate) fn derive_key(
        &self,
        branch: KeyBranch,
        index: DerivedKeyIndex,
    ) -> Result<DerivedWalletKey, KeyManagerApiError> {
        let km = self.get_key_manager(branch)?;
        let key = km
            .derive_key(index)
            .expect("derive_key only panics if the hasher does not produce 32 bytes");
        Ok(key.into())
    }

    pub fn derive_keypair(
        &self,
        branch: KeyBranch,
        key_index: DerivedKeyIndex,
    ) -> Result<DerivedKeyPair, KeyManagerApiError> {
        let derived_key = self.derive_key(branch, key_index)?;
        Ok(DerivedKeyPair {
            public_key: derived_key.to_public_key(),
            derived_key,
        })
    }

    pub fn derive_account_key(&self, index: DerivedKeyIndex) -> Result<DerivedWalletKey, KeyManagerApiError> {
        self.derive_key(KeyBranch::Account, index)
    }

    pub fn derive_account_address(
        &self,
        index: DerivedKeyIndex,
    ) -> Result<WalletOotleAddressWithKeyIds, KeyManagerApiError> {
        let key = self.derive_account_key(index)?;
        let view_only_key = self.derive_view_only_key(index)?;
        Ok(WalletOotleAddressWithKeyIds {
            address: RistrettoOotleAddress {
                network: self.network,
                view_only_key: RistrettoPublicKey::from_secret_key(&view_only_key.key),
                account_key: RistrettoPublicKey::from_secret_key(&key.key),
            },
            view_only_key_id: key.as_key_id(),
            owner_key_id: key.as_key_id(),
        })
    }

    pub fn next_account_address(&self) -> Result<WalletOotleAddressWithKeyIds, KeyManagerApiError> {
        let key = self.next_key(KeyBranch::Account)?;
        self.derive_account_address(key.key_index)
    }

    pub fn derive_view_only_key(&self, index: DerivedKeyIndex) -> Result<DerivedWalletKey, KeyManagerApiError> {
        self.derive_key(KeyBranch::ViewOnlyKey, index)
    }

    pub fn derive_view_only_keypair(&self, index: u64) -> Result<DerivedKeyPair, KeyManagerApiError> {
        let key = self.derive_view_only_key(index)?;
        let public_key = RistrettoPublicKey::from_secret_key(&key.key);
        Ok(DerivedKeyPair {
            public_key,
            derived_key: key,
        })
    }

    pub fn derive_account_key_pair(&self, index: u64) -> Result<DerivedKeyPair, KeyManagerApiError> {
        let key = self.derive_account_key(index)?;
        let public_key = RistrettoPublicKey::from_secret_key(&key.key);
        Ok(DerivedKeyPair {
            public_key,
            derived_key: key,
        })
    }

    pub fn last_index(&self, branch: &str) -> Result<u64, KeyManagerApiError> {
        let mut tx = self.store.create_read_tx()?;
        Ok(tx.key_manager_get_last_index(branch).optional()?.unwrap_or(0))
    }

    /// Derives the next key in the specified branch, increments the index, and sets it as the active key.
    /// If the branch does not exist, it will be created with index 0 and the first key will be returned.
    /// TODO: if there is another active DB transaction this function will block until it can acquire it.
    pub fn next_key(&self, branch: KeyBranch) -> Result<DerivedWalletKey, KeyManagerApiError> {
        let next_key_id = self.next_derived_key_index(branch)?;
        let key = self.derive_key(branch, next_key_id)?;
        Ok(key)
    }

    /// Derives the next key in the specified branch, increments the index, and sets it as the active key.
    /// If the branch does not exist, it will be created with index 0 and the first key will be returned.
    /// TODO: if there is another active DB transaction this function will block until it can acquire it.
    pub fn next_public_key(&self, branch: KeyBranch) -> Result<WalletPublicKey, KeyManagerApiError> {
        let next_key_id = self.next_derived_key_index(branch)?;
        let key = self.derive_key(branch, next_key_id)?;
        Ok(WalletPublicKey {
            public_key: key.to_public_key(),
            key_id: key.as_key_id(),
        })
    }

    pub fn next_derived_key_index(&self, branch: KeyBranch) -> Result<DerivedKeyIndex, KeyManagerApiError> {
        let mut tx = self.store.create_write_tx()?;
        let next_index = tx
            .key_manager_get_last_index(branch.as_str())
            .optional()?
            .map(|i| i + 1)
            .unwrap_or(0);
        if matches!(branch, KeyBranch::Account) {
            // Ensure the view key branch is created if it doesn't exist
            tx.key_manager_insert_or_ignore(KeyBranch::ViewOnlyKey.as_str(), next_index)?;
        }
        tx.key_manager_insert_or_ignore(branch.as_str(), next_index)?;
        tx.commit()?;
        Ok(next_index)
    }

    pub fn create_throwaway_nonce(&self) -> RistrettoSecretKey {
        RistrettoSecretKey::random(&mut OsRng)
    }

    pub fn set_active_key<B: AsRef<str>>(&self, branch: B, index: u64) -> Result<(), KeyManagerApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.key_manager_set_active_index(branch.as_ref(), index)?;
        tx.commit()?;
        Ok(())
    }

    /// Resets the active key index to the provided index for the given branch.
    /// A subsequent call to next_key will return the key for index + 1.
    /// If the active key is after the provided index, no key will be active.
    pub fn reset_key_index_to<B: AsRef<str>>(&self, branch: B, index: u64) -> Result<(), KeyManagerApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.key_manager_reset_index(branch.as_ref(), index)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_active_key(&self, branch: KeyBranch) -> Result<DerivedWalletKey, KeyManagerApiError> {
        let key_index = self
            .store
            .with_read_tx(|tx| tx.key_manager_get_active_index(branch.as_str()))
            .optional()?
            .unwrap_or(0);
        self.derive_key(branch, key_index)
    }

    pub fn get_key_or_active(
        &self,
        branch: KeyBranch,
        maybe_key_id: Option<KeyId>,
    ) -> Result<WalletPublicKey, KeyManagerApiError> {
        match maybe_key_id {
            Some(id) => Ok(self.get_public_key(branch, id)?),
            None => {
                let key = self.get_active_key(branch)?;
                Ok(key.into())
            },
        }
    }

    fn get_key_manager<B: AsRef<str>>(&self, branch: B) -> Result<WalletKeyManager, KeyManagerApiError> {
        let cipher_seed = self.cipher_seed.cipher_seed().ok_or(KeyManagerApiError::ReadOnlyMode)?;
        // We dont ever use the index in the key manager i.e. we dont ever call next_key on it, instead we always use
        // derive_key
        Ok(WalletKeyManager::from(
            cipher_seed.clone(),
            branch.as_ref().to_string(),
            0,
        ))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KeyManagerApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Key manager error: {0}")]
    KeyManagerError(#[from] key_manager::error::KeyManagerServiceError),
    #[error("Key {key_id} not found")]
    KeyNotFound { key_id: KeyId },
    #[error("Password manager error: {0}")]
    PasswordManagerApiError(#[from] PasswordManagerApiError),
    #[error("Key manager is in read only mode")]
    ReadOnlyMode,
}

impl IsNotFoundError for KeyManagerApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, KeyManagerApiError::KeyNotFound { .. }) ||
            matches!(self, KeyManagerApiError::StoreError(e) if e.is_not_found_error())
    }
}
