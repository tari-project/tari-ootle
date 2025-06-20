//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use anyhow::anyhow;
use async_trait::async_trait;
use tokio::process::Command;

use crate::process_definitions::{ProcessContext, ProcessDefinition};

pub const WALLET_DAEMON_AUTH_SETTINGS_KEY: &str = "wallet_daemon_auth";
pub const WALLET_DAEMON_SEED_WORDS_SETTINGS_KEY: &str = "wallet_daemon_seed_words";
const OVERRIDE_KEYRING_PASSWORD_SETTINGS_KEY: &str = "override_keyring_password";
const ARGS_SETTINGS_KEY: &str = "args";
const WALLET_DAEMON_AUTH_DEFAULT: &str = "none";

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

        let json_rpc_public_url = context.get_public_json_rpc_url();
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

        let auth = context
            .get_setting(WALLET_DAEMON_AUTH_SETTINGS_KEY)
            .or_else(|| context.get_setting("auth_method"))
            .unwrap_or(WALLET_DAEMON_AUTH_DEFAULT);

        command
            .envs(context.environment())
            .arg("-b")
            .arg(context.base_path())
            .arg("--network")
            .arg(context.network().to_string())
            .arg(format!("--json-rpc-address={json_rpc_address}"))
            .arg(format!("--indexer-url={indexer_url}"))
            .arg(format!("--web-ui-public-json-rpc-url={json_rpc_public_url}"))
            .arg(format!("-pootle_wallet_daemon.web_ui_address={web_ui_address}"))
            .arg(format!("-pootle_wallet_daemon.authentication={auth}"));

        if let Some(seed_words) = context.get_setting(WALLET_DAEMON_SEED_WORDS_SETTINGS_KEY) {
            command.arg(format!("--seed-words={seed_words}"));
        }

        if let Some(override_keyring_password) = context.get_setting(OVERRIDE_KEYRING_PASSWORD_SETTINGS_KEY) {
            command.arg(format!("--override-keyring-password='{override_keyring_password}'"));
        }

        if let Some(args) = context.get_setting(ARGS_SETTINGS_KEY) {
            for arg in args.split_whitespace() {
                command.arg(arg);
            }
        }

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
