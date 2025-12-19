//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::PeerAddress;
use tari_state_store_rocksdb::RocksDbReadOnlyStateStore;

use crate::{
    config::Config,
    helpers::open_db,
    webserver::{error::WebError, handlers::slugify_type_name},
};

#[derive(Debug, Clone)]
pub struct HandlerContext {
    config: Config,
    registered_cfs: Vec<String>,
}

impl HandlerContext {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            registered_cfs: vec![],
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn register_cf<CF>(&mut self, cf: CF) -> &mut Self {
        self.registered_cfs.push(slugify_type_name(cf));
        self
    }

    pub fn registered_cfs(&self) -> &[String] {
        &self.registered_cfs
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
