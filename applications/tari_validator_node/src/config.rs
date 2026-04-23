//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
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

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use config::Config;
use serde::{Deserialize, Serialize};
use tari_common::{ConfigurationError, DefaultConfigLoader, SubConfigPath, configuration::CommonConfig};
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_ootle_app_utilities::{
    epoch_oracle_config::EpochOracleConfig,
    p2p_config::{P2pConfig, PeerSeedsConfig, RpcConfig},
};
use tari_ootle_common_types::Network;

use crate::state_store_template_provider::TemplateConfig;

#[derive(Debug, Clone)]
pub struct ApplicationConfig {
    pub common: CommonConfig,
    pub validator_node: ValidatorNodeConfig,
    pub epoch_oracle: EpochOracleConfig,
    pub peer_seeds: PeerSeedsConfig,
    pub network: Network,
}

impl ApplicationConfig {
    pub fn load_from(cfg: &Config) -> Result<Self, ConfigurationError> {
        let mut config = Self {
            common: CommonConfig::load_from(cfg)?,
            validator_node: ValidatorNodeConfig::load_from(cfg)?,
            epoch_oracle: EpochOracleConfig::load_from(cfg)?,
            peer_seeds: PeerSeedsConfig::load_from(cfg)?,
            network: cfg.get("network")?,
        };
        config.validator_node.set_base_path(config.common.base_path());
        Ok(config)
    }

    pub fn get_layer_one_transaction_base_path(&self) -> PathBuf {
        if self.validator_node.layer_one_transaction_path.is_absolute() {
            return self.validator_node.layer_one_transaction_path.clone();
        }
        self.common
            .base_path()
            .join(&self.validator_node.layer_one_transaction_path)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct ValidatorNodeConfig {
    override_from: Option<String>,
    pub shard_key_file: PathBuf,
    /// A path to the file that stores your node identity and secret key
    pub identity_file: PathBuf,
    /// The relative path to store persistent data
    pub data_dir: PathBuf,
    /// An absolute or relative (to data_dir) path to the state database
    pub state_db_path: PathBuf,
    /// Database config
    // pub database: tari_any_state_store::Config,
    /// The p2p configuration settings
    pub p2p: P2pConfig,
    /// P2P RPC configuration
    pub rpc: RpcConfig,
    /// JSON-RPC address of the validator node  application
    pub json_rpc_listener_address: Option<SocketAddr>,
    /// The public JSON-RPC url that the Web UI uses, if specified.
    pub json_rpc_public_url: Option<String>,
    /// The address to listen on for the Web UI
    pub web_ui_listener_address: Option<SocketAddr>,
    /// Template config
    pub templates: TemplateConfig,
    /// Fee claim public key
    pub fee_claim_public_key: RistrettoPublicKey,
    /// Create identity file if not exists
    pub dont_create_id: bool,
    /// The (optional) sidechain to run this on
    pub validator_node_sidechain_id: Option<RistrettoPublicKey>,
    /// The templates sidechain id
    pub template_sidechain_id: Option<RistrettoPublicKey>,
    /// The burnt utxo sidechain id
    pub burnt_utxo_sidechain_id: Option<RistrettoPublicKey>,
    /// The path to store layer-one transactions.
    pub layer_one_transaction_path: PathBuf,
    /// Consensus configuration
    pub consensus: ConsensusConfig,
    /// Address the admin JSON-RPC listens on. Admin methods (consensus directives, etc.) are
    /// served *only* on this address, never on the public `json_rpc_listener_address`. If
    /// unset, admin RPC is disabled entirely.
    pub admin_json_rpc_listener_address: Option<SocketAddr>,
    /// Public key authorised to sign consensus directives. If unset, all directive submissions
    /// are rejected regardless of what key they are signed with.
    pub governance_public_key: Option<RistrettoPublicKey>,
}

impl ValidatorNodeConfig {
    pub fn set_base_path<P: AsRef<Path>>(&mut self, base_path: P) {
        if !self.shard_key_file.is_absolute() {
            self.shard_key_file = base_path.as_ref().join(&self.shard_key_file);
        }
        if !self.identity_file.is_absolute() {
            self.identity_file = base_path.as_ref().join(&self.identity_file);
        }
        if !self.data_dir.is_absolute() {
            self.data_dir = base_path.as_ref().join(&self.data_dir);
        }
        if !self.state_db_path.is_absolute() {
            self.state_db_path = self.data_dir.join(&self.state_db_path);
        }
        // if !self.database.rocks_db.path.is_absolute() {
        //     self.database.rocks_db.path = self.data_dir.as_ref().join(&self.database.rocks_db.path);
        // }
        // if !self.database.sqlite.path.is_absolute() {
        //     self.database.sqlite.path = self.data_dir.as_ref().join(&self.database.sqlite.path);
        // }
    }

    pub fn get_global_db_path(&self) -> PathBuf {
        self.data_dir.join("global_storage.sqlite")
    }
}

impl Default for ValidatorNodeConfig {
    fn default() -> Self {
        Self {
            override_from: None,
            shard_key_file: PathBuf::from("shard_key.json"),
            identity_file: PathBuf::from("validator_node_id.json"),
            data_dir: PathBuf::from("data/validator_node"),
            state_db_path: PathBuf::from("rocksdb"),
            // database: tari_any_state_store::Config {
            //     database_type: AnyDatabaseType::Sqlite,
            //     rocks_db: RocksConfig { path: "rocksdb".into() },
            //     sqlite: SqliteConfig {
            //         path: "state.db".into(),
            //     },
            // },
            p2p: P2pConfig::default(),
            rpc: RpcConfig::default(),
            json_rpc_listener_address: Some("127.0.0.1:18200".parse().unwrap()),
            json_rpc_public_url: None,
            web_ui_listener_address: Some("127.0.0.1:5001".parse().unwrap()),
            templates: TemplateConfig::default(),
            // Burn your fees
            fee_claim_public_key: RistrettoPublicKey::default(),
            dont_create_id: false,
            validator_node_sidechain_id: None,
            template_sidechain_id: None,
            burnt_utxo_sidechain_id: None,
            layer_one_transaction_path: PathBuf::from("data/layer_one_transactions"),
            consensus: ConsensusConfig::default(),
            // Admin RPC is opt-in; leave both unset in the default config so existing
            // deployments don't accidentally expose a directive endpoint.
            admin_json_rpc_listener_address: None,
            governance_public_key: None,
        }
    }
}

impl SubConfigPath for ValidatorNodeConfig {
    fn main_key_prefix() -> &'static str {
        "validator_node"
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConsensusConfig {
    /// Enable proposing evictions for inactive validators. If disabled, this validator will still vote on eviction
    /// proposals from other validators, including voting in the affirmative if applicable, but will never propose
    /// evictions itself.
    pub enable_eviction_proposal: bool,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            enable_eviction_proposal: true,
        }
    }
}
