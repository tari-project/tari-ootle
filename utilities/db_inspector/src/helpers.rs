//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fs;

use anyhow::{anyhow, Context};
use tari_dan_common_types::PeerAddress;
use tari_state_store_rocksdb::{RocksDbReadOnlyStateStore, RocksDbStateStore};

use crate::config::DatabaseConfig;

pub fn open_db(db_config: &DatabaseConfig) -> anyhow::Result<RocksDbReadOnlyStateStore<PeerAddress>> {
    if !db_config.path.exists() {
        anyhow::bail!("Database path does not exist: {}", db_config.path.display());
    }
    fs::create_dir_all(&db_config.secondary_path).context("Failed to create secondary path")?;

    log::info!("Opening database at: {}", db_config.path.display());
    let db = RocksDbStateStore::open_read_only(&db_config.path, &db_config.secondary_path)
        .map_err(|e| anyhow!("Failed to open database: {}", e))?;
    Ok(db)
}
