//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fs,
    fs::File,
    io,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

use config::Config;
use log::*;
use tari_common::configuration::{ConfigOverrideProvider, Network as L1Network, loader::CONFIG_OVERRIDES_KEY};
use tari_ootle_transaction::Network;

const LOG_TARGET: &str = "tari::application::configuration";

pub fn convert_network_to_l1_network(network: &Network) -> L1Network {
    match network {
        Network::MainNet => L1Network::MainNet,
        Network::StageNet => L1Network::StageNet,
        Network::NextNet => L1Network::NextNet,
        Network::LocalNet => L1Network::LocalNet,
        Network::Igor => L1Network::Igor,
        Network::Esmeralda => L1Network::Esmeralda,
    }
}

/// Loads the configuration file from the specified path, or creates a new one with the embedded default presets if it
/// does not. This also prompts the user.
pub fn load_configuration<P: AsRef<Path>, TOverride: ConfigOverrideProvider>(
    config_path: P,
    create_if_not_exists: bool,
    overrides: &TOverride,
    network_override: Option<Network>,
) -> Result<Config, ConfigError> {
    debug!(
        target: LOG_TARGET,
        "Loading configuration file from  {}",
        config_path.as_ref().display()
    );
    if !config_path.as_ref().exists() && create_if_not_exists {
        let sources = get_default_config();
        write_config_to(&config_path, &sources)?;
    }

    load_configuration_with_overrides(config_path, overrides, network_override)
}

/// Loads the config at the given path applying all overrides.
///
/// A section's network-scoped table (e.g. `[esmeralda.epoch_oracle]`) is merged over its root keys when the section
/// loads, so the overrides are also stored under [`CONFIG_OVERRIDES_KEY`] for `merge_subconfig` to re-apply on top of
/// it. An override therefore wins over the config file whether it names the root key (`epoch_oracle.x`) or the scoped
/// one (`esmeralda.epoch_oracle.x`).
pub fn load_configuration_with_overrides<P: AsRef<Path>, TOverride: ConfigOverrideProvider>(
    config_path: P,
    overrides: &TOverride,
    network_override: Option<Network>,
) -> Result<Config, ConfigError> {
    let filename = config_path
        .as_ref()
        .to_str()
        .ok_or_else(|| ConfigError::InvalidConfigPath {
            path: config_path.as_ref().to_path_buf(),
        })?;
    let cfg = Config::builder()
        .add_source(config::File::with_name(filename))
        .add_source(
            config::Environment::with_prefix("TARI")
                .prefix_separator("_")
                .separator("__"),
        )
        .build()?;
    let network = match network_override {
        Some(network) => network,
        None => match cfg.get_string("network") {
            Ok(network) => {
                Network::from_str(&network).map_err(|e| ConfigError::new("Invalid network", Some(e.to_string())))?
            },
            Err(config::ConfigError::NotFound(_)) => {
                return Err(ConfigError::new("no network provided", None));
            },
            Err(e) => {
                return Err(ConfigError::new(
                    "Could not get network configuration",
                    Some(e.to_string()),
                ));
            },
        },
    };

    let l1_network = convert_network_to_l1_network(&network);
    let overrides = overrides.get_config_property_overrides(&l1_network);
    let reapply = replay_list(&overrides, l1_network.as_key_str());

    let mut cfg = Config::builder().add_source(cfg);
    for (key, value) in overrides {
        cfg = cfg
            .set_override(key.as_str(), value.as_str())
            .map_err(|ce| ConfigError::new("Could not override config property", Some(ce.to_string())))?;
    }
    cfg = cfg
        .set_override("network", network.to_string())
        .map_err(|ce| ConfigError::new("Could not override config property", Some(ce.to_string())))?
        .set_override(CONFIG_OVERRIDES_KEY, reapply)
        .map_err(|ce| ConfigError::new("Could not override config property", Some(ce.to_string())))?;
    let cfg = cfg
        .build()
        .map_err(|ce| ConfigError::new("Could not build config", Some(ce.to_string())))?;

    Ok(cfg)
}

/// The `key=value` entries `merge_subconfig` re-applies, later entries winning: every override as given, then each
/// override scoped to `network` copied to its root key, so `-p esmeralda.epoch_oracle.x` also sets `epoch_oracle.x`.
fn replay_list(overrides: &[(String, String)], network: &str) -> Vec<String> {
    let scope = format!("{network}.");
    let as_given = overrides.iter().map(|(key, value)| (key.to_lowercase(), value));
    let unscoped = overrides.iter().filter_map(|(key, value)| {
        let root_key = key.to_lowercase().strip_prefix(&scope)?.to_string();
        root_key.contains('.').then_some((root_key, value))
    });
    as_given
        .chain(unscoped)
        .map(|(key, value)| format!("{key}={value}"))
        .collect()
}

/// Returns the default configuration file template in parts from the embedded presets. If use_mining_config is true,
/// the base node configuration that enables mining is returned, otherwise the non-mining configuration is returned.
pub fn get_default_config() -> [&'static str; 6] {
    [
        include_str!("../config_presets/a_common.toml"),
        include_str!("../config_presets/b_peer_seeds.toml"),
        include_str!("../config_presets/c_validator_node.toml"),
        include_str!("../config_presets/d_indexer.toml"),
        include_str!("../config_presets/e_wallet_daemon.toml"),
        include_str!("../config_presets/f_epoch_oracle.toml"),
    ]
}

/// Writes a single file concatenating all the provided sources to the specified path. If the parent directory does not
/// exist, it is created. If the file already exists, it is overwritten.
pub fn write_config_to<P: AsRef<Path>>(path: P, sources: &[&str]) -> Result<(), std::io::Error> {
    if let Some(d) = path.as_ref().parent() {
        fs::create_dir_all(d)?
    };
    let mut file = File::create(path)?;
    for source in sources {
        file.write_all(source.trim().as_bytes())?;
        file.write_all(b"\n\n")?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
    #[error("Invalid config file path '{}'", .path.display())]
    InvalidConfigPath { path: PathBuf },
    #[error(transparent)]
    ConfigError(#[from] config::ConfigError),
    #[error("{cause}{}", .error.as_ref().map(|s| format!(": ({})", s)).unwrap_or_default())]
    CustomError { cause: &'static str, error: Option<String> },
}

impl ConfigError {
    pub fn new(cause: &'static str, source: Option<String>) -> Self {
        Self::CustomError { cause, error: source }
    }
}
