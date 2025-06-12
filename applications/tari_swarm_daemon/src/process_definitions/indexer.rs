//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use anyhow::anyhow;
use async_trait::async_trait;
use tokio::process::Command;

use crate::process_definitions::{ProcessContext, ProcessDefinition};

#[derive(Debug, Default)]
pub struct Indexer;

impl Indexer {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProcessDefinition for Indexer {
    async fn get_command(&self, mut context: ProcessContext<'_>) -> anyhow::Result<Command> {
        let mut command = Command::new(context.bin());
        let jrpc_port = context.get_free_port("jrpc").await?;
        let graphql_port = context.get_free_port("graphql").await?;
        let web_ui_port = context.get_free_port("web").await?;
        let listen_ip = context.listen_ip();

        let json_rpc_public_url = context.get_public_json_rpc_url();
        let graphql_public_url = context.get_public_graphql_url();
        let json_rpc_listener_address = format!("{listen_ip}:{jrpc_port}");
        let graphql_listener_address = format!("{listen_ip}:{graphql_port}");
        let web_ui_listener_address = format!("{listen_ip}:{web_ui_port}");

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

        command
            .envs(context.environment())
            .arg("-b")
            .arg(context.base_path())
            .arg("--network")
            .arg(context.network().to_string())
            .arg(format!(
                "-pepoch_oracle.base_layer.base_node_grpc_url={base_node_grpc_url}"
            ))
            .arg(format!("-pindexer.json_rpc_address={json_rpc_listener_address}"))
            .arg(format!("-pindexer.graphql_address={graphql_listener_address}"))
            .arg(format!("-pindexer.web_ui_address={web_ui_listener_address}"))
            .arg(format!("-pindexer.web_ui_public_json_rpc_url={json_rpc_public_url}"))
            .arg(format!("-pindexer.web_ui_public_graphql_url={graphql_public_url}"))
            .arg("-pepoch_oracle.base_layer.scanning_interval=1");

        // mDNS is not guaranteed to work, so we'll explicitly set the seed peers for the indexer.
        let seed_peers = context.validator_nodes().filter_map(|vn| {
            let port = vn.instance().allocated_ports().get("p2p")?;
            Some(format!("/ip4/127.0.0.1/tcp/{port}"))
        });

        for seed in seed_peers {
            command.arg(format!("--peer-seeds={seed}"));
        }

        Ok(command)
    }

    fn get_relative_data_path(&self) -> Option<PathBuf> {
        Some("data".into())
    }
}
