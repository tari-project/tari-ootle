//  Copyright 2023. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use config::Config;
use serde::{Deserialize, Serialize};
use tari_common::{
    ConfigurationError,
    DefaultConfigLoader,
    SubConfigPath,
    configuration::{CommonConfig, serializers},
};
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_ootle_app_utilities::{
    epoch_oracle_config::EpochOracleConfig,
    p2p_config::{P2pConfig, PeerSeedsConfig},
};
use tari_ootle_common_types::Network;

use crate::network_state_sync::EventFilter;
use crate::rest_api::rate_limit::RateLimitConfig;

#[derive(Debug, Clone)]
pub struct ApplicationConfig {
    pub common: CommonConfig,
    pub indexer: IndexerConfig,
    pub peer_seeds: PeerSeedsConfig,
    pub epoch_oracle: EpochOracleConfig,
    pub network: Network,
}

impl ApplicationConfig {
    pub fn load_from(cfg: &Config) -> Result<Self, ConfigurationError> {
        let config = Self {
            common: CommonConfig::load_from(cfg)?,
            indexer: IndexerConfig::load_from(cfg)?,
            peer_seeds: PeerSeedsConfig::load_from(cfg)?,
            epoch_oracle: EpochOracleConfig::load_from(cfg)?,
            network: cfg.get("network")?,
        };
        Ok(config)
    }

    pub fn to_identity_file_path(&self) -> PathBuf {
        if self.indexer.identity_file.is_absolute() {
            return self.indexer.identity_file.clone();
        }

        self.common.base_path.join(&self.indexer.identity_file)
    }

    pub fn to_data_dir(&self) -> PathBuf {
        if self.indexer.data_dir.is_absolute() {
            return self.indexer.data_dir.clone();
        }

        self.common.base_path.join(&self.indexer.data_dir)
    }

    pub fn state_db_path(&self) -> PathBuf {
        self.to_data_dir().join("state.db")
    }

    pub fn global_db_path(&self) -> PathBuf {
        self.to_data_dir().join("global_storage.sqlite")
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct IndexerConfig {
    override_from: Option<String>,
    /// A path to the file that stores your node identity and secret key
    pub identity_file: PathBuf,
    /// The relative path to store persistent data
    pub data_dir: PathBuf,
    /// The p2p configuration settings
    pub p2p: P2pConfig,
    /// Listening address for the indexer API server
    pub api_listen_address: Option<SocketAddr>,
    /// GraphQL port of the indexer application
    pub graphql_address: Option<SocketAddr>,
    /// The address of the Web UI
    pub web_ui_address: Option<SocketAddr>,
    /// The publicly-accessible URL that the UI uses to connect to the API.
    /// If this is None, then the api_listen_address will be used.
    pub web_ui_public_api_url: Option<String>,
    /// The jrpc address where the UI should connect to the GraphQL API(it can be the same as the json_rpc_address, but
    /// doesn't have to be), if this will be None, then the listen_addr will be used.
    pub web_ui_public_graphql_url: Option<String>,
    /// How often do we want to scan the second layer for new versions
    #[serde(with = "serializers::seconds")]
    pub block_scanning_interval: Duration,
    #[serde(with = "serializers::seconds")]
    pub state_scanning_interval: Duration,
    /// The sidechain to listen on.
    pub sidechain_id: Option<RistrettoPublicKey>,
    /// The templates sidechain id
    pub templates_sidechain_id: Option<RistrettoPublicKey>,
    /// The burnt utxos sidechain id
    pub burnt_utxo_sidechain_id: Option<RistrettoPublicKey>,
    /// The event filtering configuration
    pub event_filters: Vec<EventFilter>,
    /// Rate limiting configuration for the REST API
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            override_from: None,
            identity_file: PathBuf::from("indexer_id.json"),
            data_dir: PathBuf::from("data/indexer"),
            p2p: P2pConfig::default(),
            api_listen_address: Some("127.0.0.1:18300".parse().unwrap()),
            graphql_address: Some("127.0.0.1:18301".parse().unwrap()),
            web_ui_address: Some("127.0.0.1:15000".parse().unwrap()),
            web_ui_public_api_url: None,
            web_ui_public_graphql_url: None,
            block_scanning_interval: Duration::from_secs(10),
            state_scanning_interval: Duration::from_secs(60),
            sidechain_id: None,
            templates_sidechain_id: None,
            burnt_utxo_sidechain_id: None,
            event_filters: vec![],
            rate_limit: RateLimitConfig::default(),
        }
    }
}

impl SubConfigPath for IndexerConfig {
    fn main_key_prefix() -> &'static str {
        "indexer"
    }
}
