//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use blake2::Blake2b;
use digest::consts::U64;
use tari_bor::{Deserialize, Serialize};
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_engine_types::ToByteType;
use tari_key_manager::{
    cipher_seed::CipherSeed,
    key_manager::{DerivedKey, KeyManager},
};
use tari_ootle_common_types::optional::{IsNotFoundError, Optional};
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

use crate::{
    models::WalletKey,
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

pub type WalletKeyManager = KeyManager<RistrettoPublicKey, Blake2b<U64>>;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[serde(rename_all = "snake_case")]
pub enum KeyBranch {
    /// The account key branch, used for deriving account keys.
    Account,
    /// The transaction key branch, used to sign transactions that do not need to be signed with the account key.
    Transaction,
    /// The view key branch, used to derive a view key for resources.
    ViewKey,
    /// The stealth masks branch, used to derive masks for stealth addresses.
    StealthMasks,
    /// The confidential masks branch, used to derive masks for confidential transactions.
    ConfidentialMasks,
}

impl KeyBranch {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Transaction => "transactions",
            Self::ViewKey => "view_key",
            Self::StealthMasks => "stealth_masks",
            Self::ConfidentialMasks => "confidential_masks",
        }
    }
}

impl AsRef<str> for KeyBranch {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

pub struct KeyManagerApi<'a, TStore> {
    store: &'a TStore,
    cipher_seed: &'a CipherSeed,
}

impl<'a, TStore: WalletStore> KeyManagerApi<'a, TStore> {
    pub(crate) fn new(store: &'a TStore, cipher_seed: &'a CipherSeed) -> Self {
        Self { store, cipher_seed }
    }

    pub fn get_or_create_initial<T: AsRef<str>>(&self, branch: T) -> Result<(), KeyManagerApiError> {
        let mut tx = self.store.create_write_tx()?;
        if tx.key_manager_get_active_index(branch.as_ref()).optional()?.is_none() {
            tx.key_manager_insert(branch.as_ref(), 0)?;
            tx.commit()?;
        } else {
            tx.rollback()?;
        }
        Ok(())
    }

    pub fn get_all_keys<B: AsRef<str>>(&self, branch: B) -> Result<Vec<WalletKey>, KeyManagerApiError> {
        let mut tx = self.store.create_read_tx()?;
        let all_keys = tx.key_manager_get_all(branch.as_ref())?;
        let mut keys = Vec::with_capacity(all_keys.len());
        for (index, active) in all_keys {
            let km = self.get_key_manager(branch.as_ref(), index);
            let key = km
                .derive_key(index)
                .map_err(tari_key_manager::error::KeyManagerError::from)?;
            let pk = RistrettoPublicKey::from_secret_key(&key.key);
            keys.push(WalletKey {
                branch: branch.as_ref().to_string(),
                public_key: pk.to_byte_type(),
                secret_key: key,
                is_active: active,
            });
        }
        Ok(keys)
    }

    pub fn derive_key<B: AsRef<str>>(
        &self,
        branch: B,
        index: u64,
    ) -> Result<DerivedKey<RistrettoPublicKey>, KeyManagerApiError> {
        let km = self.get_or_create_key_manager(branch)?;
        let key = km
                .derive_key(index)
                // TODO: Key manager shouldn't return other errors
                .map_err(tari_key_manager::error::KeyManagerError::from)?;
        Ok(key)
    }

    pub fn derive_account_keypair(
        &self,
        index: u64,
    ) -> Result<(DerivedKey<RistrettoPublicKey>, RistrettoPublicKey), KeyManagerApiError> {
        let key = self.derive_account_key(index)?;
        let public_key = RistrettoPublicKey::from_secret_key(&key.key);
        Ok((key, public_key))
    }

    pub fn derive_account_key(&self, index: u64) -> Result<DerivedKey<RistrettoPublicKey>, KeyManagerApiError> {
        self.derive_key(KeyBranch::Account, index)
    }

    pub fn next_account_key(&self) -> Result<DerivedKey<RistrettoPublicKey>, KeyManagerApiError> {
        self.next_key(KeyBranch::Account)
    }

    pub fn last_index(&self, branch: &str) -> Result<u64, KeyManagerApiError> {
        let mut tx = self.store.create_read_tx()?;
        Ok(tx.key_manager_get_last_index(branch).optional()?.unwrap_or(0))
    }

    pub fn next_key<B: AsRef<str>>(&self, branch: B) -> Result<DerivedKey<RistrettoPublicKey>, KeyManagerApiError> {
        let mut tx = self.store.create_write_tx()?;
        let index = tx.key_manager_get_last_index(branch.as_ref()).optional()?.unwrap_or(0);
        let mut key_manager = WalletKeyManager::from(self.cipher_seed.clone(), branch.as_ref().to_string(), index);
        let key = key_manager
            .next_key()
            // TODO: Key manager shouldn't return other errors
            .map_err(tari_key_manager::error::KeyManagerError::from)?;
        tx.key_manager_insert(&key_manager.branch_seed, key_manager.key_index())?;
        tx.commit()?;
        Ok(key)
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

    pub fn get_active_key<B: AsRef<str>>(
        &self,
        branch: B,
    ) -> Result<(u64, DerivedKey<RistrettoPublicKey>), KeyManagerApiError> {
        let index = self
            .store
            .with_read_tx(|tx| tx.key_manager_get_active_index(branch.as_ref()))
            .optional()?
            .unwrap_or(0);
        Ok((index, self.derive_key(branch, index)?))
    }

    pub fn get_key_or_active<B: AsRef<str>>(
        &self,
        branch: B,
        maybe_index: Option<u64>,
    ) -> Result<(u64, DerivedKey<RistrettoPublicKey>), KeyManagerApiError> {
        match maybe_index {
            Some(index) => Ok((index, self.derive_key(branch, index)?)),
            None => self.get_active_key(branch),
        }
    }

    /// Brute force key search
    /// WARNING: searching from 0 to u64::MAX will take in excess of 584942 years (assuming 1 microsecond per search)
    /// for a key not to be found. Do not use this unless you are using a small range.
    pub fn search_for_key_within_range(
        &self,
        branch: &str,
        public_key: &RistrettoPublicKeyBytes,
        start_index: u64,
        end_index: u64,
    ) -> Result<(u64, DerivedKey<RistrettoPublicKey>), KeyManagerApiError> {
        let km = self.get_or_create_key_manager(branch)?;
        for index in start_index..=end_index {
            let key = km
                .derive_key(index)
                .map_err(tari_key_manager::error::KeyManagerError::from)?;
            if RistrettoPublicKey::from_secret_key(&key.key).as_bytes() == public_key.as_bytes() {
                return Ok((index, key));
            }
        }
        // For huge search ranges, it would take many years to get here!
        Err(KeyManagerApiError::KeyNotFound {
            key: *public_key,
            branch: branch.to_string(),
        })
    }

    pub fn get_public_key(
        &self,
        branch: &str,
        key_index: Option<u64>,
    ) -> Result<RistrettoPublicKey, KeyManagerApiError> {
        let (_, key) = self.get_key_or_active(branch, key_index)?;
        Ok(RistrettoPublicKey::from_secret_key(&key.key))
    }

    fn get_or_create_key_manager<K: AsRef<str>>(&self, branch: K) -> Result<WalletKeyManager, KeyManagerApiError> {
        let branch_str = branch.as_ref();
        let mut tx = self.store.create_write_tx()?;
        let index = match tx.key_manager_get_active_index(branch_str).optional()? {
            Some(index) => {
                tx.rollback()?;
                index
            },
            None => {
                tx.key_manager_insert(branch_str, 0)?;
                tx.commit()?;
                0
            },
        };
        Ok(self.get_key_manager(branch_str, index))
    }

    fn get_key_manager<B: AsRef<str>>(&self, branch: B, index: u64) -> WalletKeyManager {
        KeyManager::from(self.cipher_seed.clone(), branch.as_ref().to_string(), index)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KeyManagerApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Key manager error: {0}")]
    KeyManagerError(#[from] tari_key_manager::error::KeyManagerError),
    #[error("Key for public key {key}, branch {branch} not found")]
    KeyNotFound {
        key: RistrettoPublicKeyBytes,
        branch: String,
    },
}

impl IsNotFoundError for KeyManagerApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, KeyManagerApiError::KeyNotFound { .. }) ||
            matches!(self, KeyManagerApiError::StoreError(e) if e.is_not_found_error())
    }
}
