//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::{Display, Formatter},
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, anyhow, bail};
use tari_bor::{Deserialize, Serialize};
use tari_common::SubConfigPath;
use tari_epoch_oracles::configured;
use tari_ootle_common_types::displayable::Displayable;
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
    Hybrid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BaseLayerOracleConfig {
    /// The Tari base node's GRPC URL. If none is specified, http://localhost:{port} will be used where {port} is the network-specific default.
    pub base_node_grpc_url: Option<Url>,
    /// Interval between base layer scans in seconds.
    #[serde(with = "ootle_serde::duration::seconds")]
    pub scanning_interval: Duration,
    /// Height to start base layer scanning
    pub start_height: u64,
}

impl Default for BaseLayerOracleConfig {
    fn default() -> Self {
        Self {
            base_node_grpc_url: None,
            // NOTE: this essentially sets the maximum possible lag (assuming the base node is perfectly in sync) for
            // epoch change. Therefore, the `epoch_end_grace_period` in consensus settings should always be
            // greater than this value to avoid a race condition resulting in leader failure.
            scanning_interval: Duration::from_secs(8),
            start_height: 0,
        }
    }
}

impl Display for BaseLayerOracleConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "url[{}], {:.2?}, start[{}]",
            self.base_node_grpc_url.as_ref().display(),
            self.scanning_interval,
            self.start_height
        )
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConfiguredOracleConfig {
    /// Absolute or relative (to current working dir) path to the ConfiguredEpochOracle config file (toml, json or
    /// json5). Mutually exclusive with `config_json`.
    #[serde(default)]
    pub config_file: Option<PathBuf>,
    /// Inline JSON (json5-compatible) configuration for the ConfiguredEpochOracle, embedded directly in the node
    /// config instead of a separate file. Mutually exclusive with `config_file`.
    #[serde(default)]
    pub config_json: Option<String>,
}

impl ConfiguredOracleConfig {
    pub async fn load(&self) -> anyhow::Result<configured::Config> {
        let config_file = match (self.config_file.as_ref(), self.config_json.as_deref()) {
            (Some(_), Some(_)) => {
                bail!("Both `config_file` and `config_json` are set for the configured oracle; set only one")
            },
            (None, None) => {
                bail!("No configured oracle source set: specify either `config_file` or `config_json`")
            },
            (None, Some(json)) => {
                return serde_json5::from_str(json).context("Failed to parse inline oracle `config_json`");
            },
            (Some(config_file), None) => config_file,
        };
        let ext = config_file
            .extension()
            .ok_or_else(|| anyhow!("Oracle config_file {} is not a file", config_file.display()))?;
        let mut file = File::open(config_file)
            .await
            .with_context(|| format!("Failed to open config file {}", config_file.display()))?;
        let config = match ext.to_str().context("extension not utf8")? {
            "toml" => {
                let mut s = String::with_capacity(1024);
                file.read_to_string(&mut s)
                    .await
                    .context("Failed to read oracle config file")?;
                toml::from_str(&s).context("Failed to parse oracle TOML config file")?
            },
            "json" | "json5" => {
                let mut file = file.into_std().await;
                serde_json5::from_reader(&mut file).context("Failed to parse oracle JSON config file")?
            },
            ext => bail!("Failed to load oracle config. Unsupported file extension {}", ext),
        };

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use config::{Config, File, FileFormat};
    use tari_common::DefaultConfigLoader;

    use super::*;

    /// The embedded epoch oracle preset, which defines the `[esmeralda.epoch_oracle]` network override.
    const PRESET: &str = include_str!("../config_presets/f_epoch_oracle.toml");

    fn load(override_from: Option<&str>) -> EpochOracleConfig {
        let mut builder = Config::builder().add_source(File::from_str(PRESET, FileFormat::Toml));
        if let Some(network) = override_from {
            builder = builder.set_override("epoch_oracle.override_from", network).unwrap();
        }
        let cfg = builder.build().unwrap();
        EpochOracleConfig::load_from(&cfg).unwrap()
    }

    #[tokio::test]
    async fn esmeralda_override_loads_inline_config_json() {
        let config = load(Some("esmeralda"));
        assert!(matches!(config.oracle_type, EpochOracleType::Hybrid));
        assert!(config.configured.config_json.is_some());
        assert!(config.configured.config_file.is_none());

        let oracle = config.configured.load().await.unwrap();
        assert_eq!(oracle.validators.len(), 4);
        assert_eq!(oracle.epoch_time, Some(Duration::from_secs(1200)));
    }

    #[tokio::test]
    async fn without_override_falls_back_to_base_layer_default() {
        // Without `epoch_oracle.override_from` set, the `[esmeralda.epoch_oracle]` section must not be applied.
        let config = load(None);
        assert!(matches!(config.oracle_type, EpochOracleType::BaseLayer));
        assert!(config.configured.config_json.is_none());
    }

    #[tokio::test]
    async fn inline_config_json_loads() {
        let configured = ConfiguredOracleConfig {
            config_file: None,
            config_json: Some(
                r#"{ "initial_epoch": 7, "base_time": "2026-02-23T13:08:42+0000", "validators": [] }"#.to_string(),
            ),
        };
        let oracle = configured.load().await.unwrap();
        assert_eq!(oracle.initial_epoch, tari_ootle_common_types::Epoch(7));
    }

    #[tokio::test]
    async fn setting_both_sources_is_an_error() {
        let configured = ConfiguredOracleConfig {
            config_file: Some(PathBuf::from("oracle.toml")),
            config_json: Some(r#"{ "initial_epoch": 7, "base_time": "2026-02-23T13:08:42+0000" }"#.to_string()),
        };
        assert!(configured.load().await.is_err());
    }

    #[tokio::test]
    async fn setting_no_source_is_an_error() {
        let configured = ConfiguredOracleConfig::default();
        assert!(configured.load().await.is_err());
    }
}
