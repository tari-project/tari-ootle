//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::process::Command;

use crate::process_definitions::{wallet_daemon::WalletDaemon, ProcessContext, ProcessDefinition};

#[derive(Debug, Default)]
pub struct WalletDaemonCreateKey;

impl WalletDaemonCreateKey {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProcessDefinition for WalletDaemonCreateKey {
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
                "create-key",
                "--key",
                "0",
                "--set-active",
                "--output",
                output_path
                    .to_str()
                    .expect("Non-UTF8 output path in WalletDaemonCreateKey"),
            ]);

        Ok(command)
    }

    fn get_relative_data_path(&self) -> Option<PathBuf> {
        WalletDaemon::new().get_relative_data_path()
    }
}
