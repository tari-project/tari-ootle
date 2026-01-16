//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{de::DeserializeOwned, Serialize};
use tari_ootle_common_types::NodeAddressable;
use tari_ootle_storage::global::GlobalDb;
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;

pub trait EpochOracleStore {
    fn get<T: DeserializeOwned>(&self, key: &[u8]) -> anyhow::Result<Option<T>>;
    fn set<T: Serialize>(&self, key: &[u8], value: &T) -> anyhow::Result<()>;
}

pub enum StoreKey {
    BaseLayerLastScannedTip,
    BaseLayerLastScannedBlockHeight,
    BaseLayerLastScannedBlockHash,
    BaseLayerLastEpochHash,
    ConfiguredIsInitialized,
    ConfiguredCurrentEpoch,
}

impl StoreKey {
    pub fn as_key_bytes(&self) -> &'static [u8] {
        match self {
            Self::BaseLayerLastScannedTip => b"base_layer.last_scanned_tip",
            Self::BaseLayerLastScannedBlockHash => b"base_layer.last_scanned_block_hash",
            Self::BaseLayerLastScannedBlockHeight => b"base_layer.last_scanned_block_height",
            Self::BaseLayerLastEpochHash => b"base_layer.last_epoch_hash",
            Self::ConfiguredIsInitialized => b"configured_oracle.is_initialized",
            Self::ConfiguredCurrentEpoch => b"configured_oracle.current_epoch",
        }
    }
}

impl<TAddr: NodeAddressable> EpochOracleStore for GlobalDb<SqliteGlobalDbAdapter<TAddr>> {
    fn get<T: DeserializeOwned>(&self, key: &[u8]) -> anyhow::Result<Option<T>> {
        let mut tx = self.create_transaction()?;
        let val = self.metadata(&mut tx).get_metadata(key)?;
        Ok(val)
    }

    fn set<T: Serialize>(&self, key: &[u8], value: &T) -> anyhow::Result<()> {
        let mut tx = self.create_transaction()?;
        self.metadata(&mut tx).set_metadata(key, value)?;
        tx.commit()?;
        Ok(())
    }
}
