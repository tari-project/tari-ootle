//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use blake2::Blake2b;
use digest::consts::U64;
use tari_bor::{Deserialize, Serialize};
use tari_common_types::seeds::cipher_seed::CipherSeed;
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    Network,
};
use tari_ootle_wallet_crypto::RistrettoOotleAddress;
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;
use tari_transaction_components::{
    key_manager,
    key_manager::tari_key_manager::{DerivedKey, TariKeyManager},
};

use crate::{
    models::{DerivedAddress, KeyPair, WalletKey},
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

pub type WalletKeyManager = TariKeyManager<Blake2b<U64>>;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-daemon-client/"))]
#[serde(rename_all = "snake_case")]
pub enum KeyBranch {
    /// The account key branch, used for deriving account keys.
    Account,
    /// The transaction key branch, used to sign transactions that do not need to be signed with the account key.
    Transaction,
    /// The Elgamal encryption view key branch, used to derive a view key for resources with "viewable balance"
    /// enabled.
    ElgamalEncryptionViewKey,
    /// The stealth mask branch, used to derive masks for stealth addresses.
    StealthMask,
    /// The confidential mask branch, used to derive masks for confidential transactions.
    ConfidentialMask,
    /// Used to generate nonces that need to be recreated later, e.g. to derive the DH secret for claim burn
    Nonce,
    /// Branch used to derive view-only keys. This key is used to derive an encryption key for wallet recovery. But
    /// does not allow spending.
    ViewOnlyKey,
}

impl KeyBranch {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Transaction => "transactions",
            Self::ElgamalEncryptionViewKey => "elgamal_view_key",
            Self::StealthMask => "stealth_mask",
            Self::ConfidentialMask => "confidential_mask",
            Self::Nonce => "nonce",
            Self::ViewOnlyKey => "view_only_key",
        }
    }
}

impl AsRef<str> for KeyBranch {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

pub struct KeyManagerApi<'a, TStore> {
    network: Network,
    store: &'a TStore,
    cipher_seed: &'a CipherSeed,
}

impl<'a, TStore: WalletStore> KeyManagerApi<'a, TStore> {
    pub(crate) fn new(network: Network, store: &'a TStore, cipher_seed: &'a CipherSeed) -> Self {
        Self {
            network,
            store,
            cipher_seed,
        }
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

    pub fn get_all_keys(&self, branch: KeyBranch) -> Result<Vec<WalletKey>, KeyManagerApiError> {
        self.get_all_keys_as_branch(branch, branch)
    }

    pub fn get_all_view_keys(&self) -> Result<Vec<WalletKey>, KeyManagerApiError> {
        // TODO: kind of hacky, but because the view key is derived from the account key index, and because we dont make
        // a view key entry for each account getting all keys for the view key branch will not return the same
        // number of keys as the account. As a workaround we get all account keys and derive the view keys from them
        self.get_all_keys_as_branch(KeyBranch::Account, KeyBranch::ViewOnlyKey)
    }

    fn get_all_keys_as_branch(
        &self,
        from_branch: KeyBranch,
        as_branch: KeyBranch,
    ) -> Result<Vec<WalletKey>, KeyManagerApiError> {
        let all_keys = self
            .store
            .with_read_tx(|tx| tx.key_manager_get_all(from_branch.as_str()))?;
        let mut keys = Vec::with_capacity(all_keys.len());
        let km = self.get_key_manager(as_branch.as_str(), 0);
        for (index, active) in all_keys {
            let key = km
                .derive_key(index)
                .map_err(key_manager::error::KeyManagerServiceError::from)?;
            let pk = RistrettoPublicKey::from_secret_key(&key.key);
            keys.push(WalletKey {
                branch: as_branch,
                key_pair: KeyPair {
                    public_key: pk,
                    secret_key: key,
                },
                is_active: active,
            });
        }
        Ok(keys)
    }

    pub fn derive_key<B: AsRef<str>>(&self, branch: B, index: u64) -> Result<DerivedKey, KeyManagerApiError> {
        let km = self.get_key_manager(branch, 0);
        let key = km
            .derive_key(index)
            .expect("derive_key only panics if the hasher does not produce 32 bytes");
        Ok(key)
    }

    pub fn derive_keypair<B: AsRef<str>>(&self, branch: B, index: u64) -> Result<KeyPair, KeyManagerApiError> {
        let key = self.derive_key(branch, index)?;
        let public_key = RistrettoPublicKey::from_secret_key(&key.key);
        Ok(KeyPair {
            public_key,
            secret_key: key,
        })
    }

    pub fn derive_account_keypair(&self, index: u64) -> Result<(DerivedKey, RistrettoPublicKey), KeyManagerApiError> {
        let key = self.derive_account_key(index)?;
        let public_key = RistrettoPublicKey::from_secret_key(&key.key);
        Ok((key, public_key))
    }

    pub fn derive_account_key(&self, index: u64) -> Result<DerivedKey, KeyManagerApiError> {
        self.derive_key(KeyBranch::Account, index)
    }

    pub fn derive_account_address(&self, index: u64) -> Result<DerivedAddress, KeyManagerApiError> {
        let key = self.derive_account_key(index)?;
        let view_only_key = self.derive_view_only_key(index)?;
        Ok(DerivedAddress {
            address: RistrettoOotleAddress {
                network: self.network,
                view_only_key: RistrettoPublicKey::from_secret_key(&view_only_key.key),
                account_key: RistrettoPublicKey::from_secret_key(&key.key),
            },
            key_index: key.key_index,
        })
    }

    pub fn next_account_address(&self) -> Result<DerivedAddress, KeyManagerApiError> {
        let key = self.next_key(KeyBranch::Account)?;
        let view_only_key = self.derive_view_only_key(key.key_index)?;
        let account_key = RistrettoPublicKey::from_secret_key(&key.key);
        let view_only_key = RistrettoPublicKey::from_secret_key(&view_only_key.key);

        Ok(DerivedAddress {
            address: RistrettoOotleAddress {
                network: self.network,
                view_only_key,
                account_key,
            },
            key_index: key.key_index,
        })
    }

    pub fn derive_view_only_key(&self, index: u64) -> Result<DerivedKey, KeyManagerApiError> {
        self.derive_key(KeyBranch::ViewOnlyKey, index)
    }

    pub fn derive_view_only_keypair(&self, index: u64) -> Result<KeyPair, KeyManagerApiError> {
        let key = self.derive_view_only_key(index)?;
        let public_key = RistrettoPublicKey::from_secret_key(&key.key);
        Ok(KeyPair {
            public_key,
            secret_key: key,
        })
    }

    pub fn derive_account_key_pair(&self, index: u64) -> Result<KeyPair, KeyManagerApiError> {
        let key = self.derive_account_key(index)?;
        let public_key = RistrettoPublicKey::from_secret_key(&key.key);
        Ok(KeyPair {
            public_key,
            secret_key: key,
        })
    }

    pub fn last_index(&self, branch: &str) -> Result<u64, KeyManagerApiError> {
        let mut tx = self.store.create_read_tx()?;
        Ok(tx.key_manager_get_last_index(branch).optional()?.unwrap_or(0))
    }

    /// Derives the next key in the specified branch, increments the index, and sets it as the active key.
    /// If the branch does not exist, it will be created with index 0 and the first key will be returned.
    /// NOTE: if there is another active DB transaction this function will block until it can acquire it.
    pub fn next_key<B: AsRef<str>>(&self, branch: B) -> Result<DerivedKey, KeyManagerApiError> {
        let mut tx = self.store.create_write_tx()?;
        let index = tx.key_manager_get_last_index(branch.as_ref()).optional()?.unwrap_or(0);
        let mut key_manager = WalletKeyManager::from(self.cipher_seed.clone(), branch.as_ref().to_string(), index);
        let key = key_manager
            .next_key()
            // TODO: Key manager shouldn't return other errors
            .map_err(key_manager::error::KeyManagerServiceError::from)?;
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

    pub fn get_active_key<B: AsRef<str>>(&self, branch: B) -> Result<(u64, DerivedKey), KeyManagerApiError> {
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
    ) -> Result<(u64, DerivedKey), KeyManagerApiError> {
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
    ) -> Result<(u64, DerivedKey), KeyManagerApiError> {
        let km = self.get_or_create_key_manager(branch)?;
        for index in start_index..=end_index {
            let key = km
                .derive_key(index)
                .map_err(key_manager::error::KeyManagerServiceError::from)?;
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
        WalletKeyManager::from(self.cipher_seed.clone(), branch.as_ref().to_string(), index)
    }
}

impl<TStore> Clone for KeyManagerApi<'_, TStore> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<TStore> Copy for KeyManagerApi<'_, TStore> {}

#[derive(Debug, thiserror::Error)]
pub enum KeyManagerApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Key manager error: {0}")]
    KeyManagerError(#[from] key_manager::error::KeyManagerServiceError),
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
