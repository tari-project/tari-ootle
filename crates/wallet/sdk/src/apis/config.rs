//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{str::FromStr, sync::OnceLock};

use serde::{de::DeserializeOwned, Serialize};
use tari_ootle_common_types::{optional::IsNotFoundError, Network};

use crate::storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter};

#[derive(Debug)]
pub struct ConfigApi<'a, TStore> {
    store: &'a TStore,
    cached_network: OnceLock<Network>,
}

impl<'a, TStore: WalletStore> ConfigApi<'a, TStore> {
    pub fn new(store: &'a TStore) -> Self {
        Self {
            store,
            cached_network: OnceLock::new(),
        }
    }

    pub fn get_network(&self) -> Result<Network, ConfigApiError> {
        if let Some(network) = self.cached_network.get() {
            return Ok(*network);
        }
        let network = self.get::<String>(ConfigKey::Network)?;
        let network = Network::from_str(&network).map_err(|e| ConfigApiError::FailedToParseNetwork {
            string: network,
            details: e.to_string(),
        })?;
        self.cached_network
            .set(network)
            .expect("Network should only be set once");
        Ok(network)
    }

    pub fn get<T>(&self, key: ConfigKey) -> Result<T, ConfigApiError>
    where T: DeserializeOwned {
        let mut tx = self.store.create_read_tx()?;
        let record = tx.config_get(key.as_key_str())?;
        // TODO: decryption if record.is_encrypted
        Ok(record.value)
    }

    pub fn exists(&self, key: ConfigKey) -> Result<bool, ConfigApiError> {
        let mut tx = self.store.create_read_tx()?;
        let exists = tx.config_exists(key.as_key_str())?;
        Ok(exists)
    }

    pub fn set<T: Serialize + ?Sized>(
        &self,
        key: ConfigKey,
        value: &T,
        is_encrypted: bool,
    ) -> Result<(), ConfigApiError> {
        let mut tx = self.store.create_write_tx()?;
        // TODO: Actually encrypt if is_encrypted is true
        tx.config_set(key.as_key_str(), value, is_encrypted)?;
        tx.commit()?;
        Ok(())
    }
}

pub enum ConfigKey {
    /// The network the wallet is running on. type: String
    Network,
    /// The cipher seed used to encrypt the wallet. type: Vec<u8>
    CipherSeed,
    /// The URL of the indexer. type: String
    IndexerUrl,
    /// Indicates whether the wallet needs to be recovered. type: bool
    RecoveryNeeded,
}

impl ConfigKey {
    pub fn as_key_str(&self) -> &'static str {
        match self {
            ConfigKey::Network => "network",
            ConfigKey::CipherSeed => "cipher_seed",
            ConfigKey::IndexerUrl => "indexer_url",
            ConfigKey::RecoveryNeeded => "recovery_needed",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Failed to parse network string '{string}': {details}")]
    FailedToParseNetwork { string: String, details: String },
}

impl IsNotFoundError for ConfigApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::StoreError(e) if e.is_not_found_error())
    }
}
