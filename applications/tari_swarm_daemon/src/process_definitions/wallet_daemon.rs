//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Display, path::PathBuf, str::FromStr};

use anyhow::anyhow;
use async_trait::async_trait;
use clap::ValueEnum;
use tokio::process::Command;

use crate::process_definitions::{ProcessContext, ProcessDefinition};

pub const WALLET_DAEMON_AUTH_SETTINGS_KEY: &str = "wallet_daemon_auth";
pub const WALLET_DAEMON_AUTH_DEFAULT: WalletDaemonAuth = WalletDaemonAuth::None;

#[derive(Clone, Debug, ValueEnum)]
pub enum WalletDaemonAuth {
    None,
    Webauthn,
}

impl FromStr for WalletDaemonAuth {
    type Err = anyhow::Error;

    fn from_str(auth_str: &str) -> Result<Self, Self::Err> {
        match auth_str {
            "none" => Ok(WalletDaemonAuth::None),
            "webauthn" => Ok(WalletDaemonAuth::Webauthn),
            _ => Err(anyhow!("Invalid auth option!")),
        }
    }
}

impl Default for WalletDaemonAuth {
    fn default() -> Self {
        Self::None
    }
}

impl Display for WalletDaemonAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            WalletDaemonAuth::None => "none".to_string(),
            WalletDaemonAuth::Webauthn => "webauthn".to_string(),
        };
        write!(f, "{}", str)
    }
}

#[derive(Debug, Default)]
pub struct WalletDaemon;

impl WalletDaemon {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProcessDefinition for WalletDaemon {
    async fn get_command(&self, mut context: ProcessContext<'_>) -> anyhow::Result<Command> {
        let mut command = Command::new(context.bin());
        let jrpc_port = context.get_free_port("jrpc").await?;
        let web_ui_port = context.get_free_port("web").await?;
        let listen_ip = context.listen_ip();
        let public_ip = context.get_setting("public_ip").unwrap_or("127.0.0.1");

        let json_rpc_public_address = format!("{public_ip}:{jrpc_port}");
        let json_rpc_address = format!("{listen_ip}:{jrpc_port}");
        let web_ui_address = format!("{listen_ip}:{web_ui_port}");

        let indexer = context
            .indexers()
            .next()
            .ok_or_else(|| anyhow!("Indexer should be started before wallet daemon"))?;
        let indexer_url = format!(
            "http://127.0.0.1:{}",
            indexer
                .instance()
                .allocated_ports()
                .get("jrpc")
                .ok_or_else(|| anyhow!("Indexer jrpc port not found"))?
        );

        let default_auth_str = WALLET_DAEMON_AUTH_DEFAULT.to_string();
        let auth = context
            .get_setting(WALLET_DAEMON_AUTH_SETTINGS_KEY)
            .unwrap_or(default_auth_str.as_str());

        command
            .envs(context.environment())
            .arg("-b")
            .arg(context.base_path())
            .arg("--network")
            .arg(context.network().to_string())
            .arg(format!("--json-rpc-address={json_rpc_address}"))
            .arg(format!("--indexer-url={indexer_url}"))
            .arg(format!("--ui-connect-address={json_rpc_public_address}"))
            .arg(format!("-pdan_wallet_daemon.web_ui_address={web_ui_address}"))
            .arg(format!("-pdan_wallet_daemon.authentication={}", auth));

        // A signaling server is not required for startup of the wallet daemon,
        // but if it is available we want to set it up
        let maybe_signaling_server = context.signaling_servers().next();
        if let Some(signaling_server) = maybe_signaling_server {
            let signaling_server_url = format!(
                "{listen_ip}:{}",
                signaling_server
                    .instance()
                    .allocated_ports()
                    .get("jrpc")
                    .ok_or_else(|| anyhow!("Signaling server port not found"))?
            );
            command.arg(format!("--signaling-server-address={signaling_server_url}"));
        }

        Ok(command)
    }

    fn get_relative_data_path(&self) -> Option<PathBuf> {
        Some("data".into())
    }
}
