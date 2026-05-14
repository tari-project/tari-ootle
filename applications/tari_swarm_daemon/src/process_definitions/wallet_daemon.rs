//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use anyhow::anyhow;
use async_trait::async_trait;
use tokio::process::Command;

use crate::process_definitions::{ARGS_SETTINGS_KEY, ProcessContext, ProcessDefinition};

pub const WALLET_DAEMON_AUTH_SETTINGS_KEY: &str = "wallet_daemon_auth";
pub const WALLET_DAEMON_SEED_WORDS_SETTINGS_KEY: &str = "wallet_daemon_seed_words";
pub const WALLET_DAEMON_INDEXER_URL_SETTINGS_KEY: &str = "indexer_url";
pub const OVERRIDE_KEYRING_PASSWORD_SETTINGS_KEY: &str = "override_keyring_password";
pub const ENABLE_VITE_DEV_SETTINGS_KEY: &str = "enable_vite_dev";
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
        // The web and JRPC ports are the same. Some other components expect a "web" port.
        context.port_allocator_mut().set_port("web", jrpc_port);
        let listen_ip = context.listen_ip();

        let json_rpc_address = format!("{listen_ip}:{jrpc_port}");
        let vite_dev_port = context
            .get_setting(ENABLE_VITE_DEV_SETTINGS_KEY)
            .and_then(|s| s.parse::<u16>().ok());

        let indexer_url = context
            .get_setting(WALLET_DAEMON_INDEXER_URL_SETTINGS_KEY)
            .map(|s| s.to_string())
            .map(anyhow::Ok)
            .unwrap_or_else(|| {
                let indexer = context
                    .indexers()
                    .next()
                    .ok_or_else(|| anyhow!("Indexer should be started before wallet daemon"))?;
                Ok(format!(
                    "http://127.0.0.1:{}",
                    indexer
                        .instance()
                        .allocated_ports()
                        .get("api")
                        .ok_or_else(|| anyhow!("Indexer api port not found"))?
                ))
            })?;

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
            .arg(format!("--listen-on={json_rpc_address}"))
            .arg(format!("--indexer-url={indexer_url}"))
            .arg(format!("-pootle_wallet_daemon.authentication={auth}"));

        if let Some(port) = vite_dev_port {
            command.arg(format!("--vite-dev={}", port));
        }

        if let Some(seed_words) = context.get_setting(WALLET_DAEMON_SEED_WORDS_SETTINGS_KEY) {
            command.arg(format!("--seed-words={seed_words}"));
        }

        if let Some(override_keyring_password) = context.get_setting(OVERRIDE_KEYRING_PASSWORD_SETTINGS_KEY) {
            command
                .arg("--override-keyring-password")
                .arg(override_keyring_password);
        }

        if let Some(args) = context.get_setting(ARGS_SETTINGS_KEY) {
            for arg in args.split_whitespace() {
                command.arg(arg);
            }
        }

        Ok(command)
    }

    fn get_relative_data_path(&self) -> Option<PathBuf> {
        Some("data".into())
    }
}
