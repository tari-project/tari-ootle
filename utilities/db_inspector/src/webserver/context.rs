//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::PeerAddress;
use tari_state_store_rocksdb::RocksDbReadOnlyStateStore;

use crate::{config::Config, helpers::open_db, webserver::error::WebError};

#[derive(Debug, Clone)]
pub struct HandlerContext {
    config: Config,
}

impl HandlerContext {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn open_db(&self, db_name: &str) -> Result<RocksDbReadOnlyStateStore<PeerAddress>, WebError> {
        let config = self
            .config()
            .get_database(db_name)
            .ok_or_else(|| WebError::bad_request(format!("Database {} not found", db_name)))?;
        let db = open_db(config)
            .map_err(|e| WebError::internal_server_error(format!("Failed to open database {}: {}", db_name, e)))?;
        Ok(db)
    }
}
