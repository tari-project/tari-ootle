//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use anyhow::anyhow;
use async_trait::async_trait;
use log::debug;
use tokio::process::Command;

use crate::process_definitions::{ProcessContext, ProcessDefinition};

#[derive(Debug, Default)]
pub struct ValidatorNode;

impl ValidatorNode {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProcessDefinition for ValidatorNode {
    async fn get_command(&self, mut context: ProcessContext<'_>) -> anyhow::Result<Command> {
        let mut command = Command::new(context.bin());
        let jrpc_port = context.get_free_port("jrpc").await?;
        let web_ui_port = context.get_free_port("web").await?;
        let p2p_port = context.get_free_port("p2p").await?;
        let listen_ip = context.listen_ip();

        let json_rpc_address = format!("{listen_ip}:{jrpc_port}");
        let json_rpc_public_url = context.get_public_json_rpc_url();
        let web_ui_address = format!("{listen_ip}:{web_ui_port}");

        let base_node = context
            .minotari_nodes()
            .next()
            .ok_or_else(|| anyhow!("Base nodes should be started before validator nodes"))?;

        let base_node_grpc_url = base_node
            .instance()
            .allocated_ports()
            .get("grpc")
            .map(|port| format!("http://127.0.0.1:{port}"))
            .ok_or_else(|| anyhow!("grpc port not found for base node"))?;

        debug!(
            "Starting validator node #{} with base node grpc address: {}",
            context.instance_id(),
            base_node_grpc_url
        );

        if let Some(claim_public_key) = context.get_setting("claim_public_key") {
            command.arg(format!("-pvalidator_node.fee_claim_public_key={claim_public_key}"));
        }

        command
            .envs(context.environment())
            .arg("-b")
            .arg(context.base_path())
            .arg("--network")
            .arg(context.network().to_string())
            .arg(format!("--json-rpc-public-url={json_rpc_public_url}"))
            .arg(format!(
                "-pepoch_oracle.base_layer.base_node_grpc_url={base_node_grpc_url}"
            ))
            .arg(format!("-pvalidator_node.p2p.listener_port={p2p_port}"))
            .arg(format!("-pvalidator_node.json_rpc_listener_address={json_rpc_address}"))
            .arg(format!("-pvalidator_node.web_ui_listener_address={web_ui_address}"));
        // .arg("-pepoch_oracle.base_layer.scanning_interval=1");

        if let Some(console_port) = context.get_setting("tokio_console_port") {
            command.arg(format!("--tokio-console-port={console_port}"));
        }

        Ok(command)
    }

    fn get_relative_data_path(&self) -> Option<PathBuf> {
        Some("data".into())
    }
}
