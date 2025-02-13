//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_bor::{Deserialize, Serialize};
use tari_common::SubConfigPath;
use tari_engine_types::serde_with;
use tari_epoch_oracles::configured;
use url::Url;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpochOracleConfig {
    override_from: Option<String>,
    #[serde(rename = "oracle")]
    pub oracle_type: EpochOracleType,
    pub base_layer: BaseLayerOracleConfig,
    pub configured: configured::Config,
}

impl SubConfigPath for EpochOracleConfig {
    fn main_key_prefix() -> &'static str {
        "epoch_oracle"
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum EpochOracleType {
    #[default]
    BaseLayer,
    Configured,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BaseLayerOracleConfig {
    /// The Tari base node's GRPC URL. If none is specified, http://localhost:{port} will be used where {port} is the network-specific default.
    pub base_node_grpc_url: Option<Url>,
    /// Interval between base layer scans
    #[serde(with = "serde_with::duration::seconds")]
    pub base_layer_scanning_interval: Duration,
}

impl Default for BaseLayerOracleConfig {
    fn default() -> Self {
        Self {
            base_node_grpc_url: None,
            base_layer_scanning_interval: Duration::from_secs(10),
        }
    }
}
