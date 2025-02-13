//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{path::PathBuf, time::Duration};

use anyhow::{anyhow, bail, Context};
use tari_bor::{Deserialize, Serialize};
use tari_common::SubConfigPath;
use tari_engine_types::serde_with;
use tari_epoch_oracles::configured;
use tokio::{fs::File, io::AsyncReadExt};
use url::Url;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpochOracleConfig {
    override_from: Option<String>,
    #[serde(rename = "oracle")]
    pub oracle_type: EpochOracleType,
    pub base_layer: BaseLayerOracleConfig,
    pub configured: ConfiguredOracleConfig,
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
    pub scanning_interval: Duration,
}

impl Default for BaseLayerOracleConfig {
    fn default() -> Self {
        Self {
            base_node_grpc_url: None,
            scanning_interval: Duration::from_secs(10),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfiguredOracleConfig {
    /// Absolute or relative (to current working dir) path to the ConfiguredEpochOracle config
    pub config_file: PathBuf,
}

impl ConfiguredOracleConfig {
    pub async fn load(&self) -> anyhow::Result<configured::Config> {
        let ext = self
            .config_file
            .extension()
            .ok_or_else(|| anyhow!("Oracle config_file {} is not a file", self.config_file.display()))?;
        let mut file = File::open(&self.config_file)
            .await
            .with_context(|| format!("Failed to open config file {}", self.config_file.display()))?;
        let config = match ext.to_str().context("extension not utf8")? {
            "toml" => {
                let mut s = String::with_capacity(1024);
                file.read_to_string(&mut s)
                    .await
                    .context("Failed to read oracle config file")?;
                toml::from_str(&s).context("Failed to parse oracle TOML config file")?
            },
            "json" => {
                let mut file = file.into_std().await;
                serde_json::from_reader(&mut file).context("Failed to parse oracle JSON config file")?
            },
            ext => bail!("Failed to load oracle config. Unsupported file extension {}", ext),
        };

        Ok(config)
    }
}

impl Default for ConfiguredOracleConfig {
    fn default() -> Self {
        Self {
            config_file: PathBuf::from("data/oracle-config.toml"),
        }
    }
}
