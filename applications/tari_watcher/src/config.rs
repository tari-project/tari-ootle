// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use tari_common::configuration::Network;
use tokio::io::{self, AsyncWriteExt};
use url::Url;

use crate::{
    cli::Cli,
    constants::{DEFAULT_BASE_NODE_GRPC_URL, DEFAULT_BASE_WALLET_GRPC_URL, DEFAULT_VALIDATOR_NODE_BINARY_PATH},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// Allow watcher to restart the validator node if it crashes and stops running
    pub network: Network,
    /// Allow watcher to submit a new validator node registration transaction initially and before
    /// the current registration expires
    pub auto_register: bool,

    /// Allow watcher to restart the validator node if it crashes and stops running
    pub auto_restart: bool,

    /// The Minotari node gRPC URL
    pub base_node_grpc_url: Url,

    /// The Minotari console wallet gRPC URL
    pub base_wallet_grpc_url: Url,

    /// The base directory of the watcher with configuration and data files
    pub base_dir: PathBuf,

    /// The path of the validator node base directory. This directory is automatically created when starting a new VN.
    pub vn_base_dir: PathBuf,

    /// The sidechain ID to use. If not provided, the default Tari sidechain ID will be used.
    pub sidechain_id: Option<String>,

    /// Path to the executable for the validator node
    pub validator_node_executable_path: PathBuf,

    /// The channel configurations for alerting and monitoring
    pub channel_config: Channels,
}

impl Config {
    pub(crate) async fn write<W: io::AsyncWrite + Unpin>(&self, mut writer: W) -> anyhow::Result<()> {
        let toml = toml::to_string_pretty(self)?;
        writer.write_all(toml.as_bytes()).await?;
        Ok(())
    }

    pub fn get_registration_file(&self) -> PathBuf {
        self.vn_base_dir
            .join(self.network.as_key_str())
            .join("registration.json")
    }

    pub fn get_layer_one_transaction_path(&self) -> PathBuf {
        self.vn_base_dir
            .join(self.network.as_key_str())
            .join("data")
            .join("layer_one_transactions")
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChannelConfig {
    pub name: String,
    pub enabled: bool,
    pub server_url: String,
    pub channel_id: String,
    pub credentials: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Channels {
    pub mattermost: ChannelConfig,
    pub telegram: ChannelConfig,
}

pub fn get_base_config(cli: &Cli) -> anyhow::Result<Config> {
    let base_dir = cli.common.base_dir.clone();
    let vn_base_dir = cli.get_validator_node_base_dir();
    let network = cli.common.network.unwrap_or(Network::Esmeralda);

    Ok(Config {
        network,
        auto_register: true,
        auto_restart: true,
        base_node_grpc_url: DEFAULT_BASE_NODE_GRPC_URL.parse()?,
        base_wallet_grpc_url: DEFAULT_BASE_WALLET_GRPC_URL.parse()?,
        base_dir: base_dir.to_path_buf(),
        sidechain_id: None,
        vn_base_dir,
        validator_node_executable_path: DEFAULT_VALIDATOR_NODE_BINARY_PATH.into(),
        channel_config: Channels {
            mattermost: ChannelConfig {
                name: "mattermost".to_string(),
                enabled: false,
                server_url: "https://some.corporation.com".to_string(),
                channel_id: "".to_string(),
                credentials: "".to_string(),
            },
            telegram: ChannelConfig {
                name: "telegram".to_string(),
                enabled: false,
                server_url: "https://api.telegram.org".to_string(),
                channel_id: "".to_string(),
                credentials: "".to_string(),
            },
        },
    })
}
