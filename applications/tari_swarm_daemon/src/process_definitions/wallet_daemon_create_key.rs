//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::process::Command;

use crate::process_definitions::{wallet_daemon, wallet_daemon::WalletDaemon, ProcessContext, ProcessDefinition};

#[derive(Debug, Default)]
pub struct WalletDaemonCreateAccount;

impl WalletDaemonCreateAccount {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProcessDefinition for WalletDaemonCreateAccount {
    async fn get_command(&self, context: ProcessContext<'_>) -> anyhow::Result<Command> {
        let mut command = Command::new(context.bin());
        let output_path = context.processes_path().join("claim_key.json");

        command
            .envs(context.environment())
            .arg("-b")
            .arg(context.base_path())
            .arg("--network")
            .arg(context.network().to_string())
            .args([
                "create-account",
                "--name",
                "Fees",
                "--key",
                "0",
                "--set-active",
                "--output",
                output_path
                    .to_str()
                    .expect("Non-UTF8 output path in WalletDaemonCreateAccount"),
            ]);

        if let Some(override_keyring_password) =
            context.get_setting(wallet_daemon::OVERRIDE_KEYRING_PASSWORD_SETTINGS_KEY)
        {
            command
                .arg("--override-keyring-password")
                .arg(override_keyring_password);
        }

        Ok(command)
    }

    fn get_relative_data_path(&self) -> Option<PathBuf> {
        WalletDaemon::new().get_relative_data_path()
    }
}
