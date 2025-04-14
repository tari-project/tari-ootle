//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::PeerAddress;
use tari_state_store_rocksdb::RocksDbReadOnlyStateStore;

use crate::{
    config::Config,
    webserver::{error::WebError, handlers::web_open_db},
};

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
        web_open_db(self, Some(db_name))
    }
}
