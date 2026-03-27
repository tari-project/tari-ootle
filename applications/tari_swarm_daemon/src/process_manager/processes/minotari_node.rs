//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fs::File;

use anyhow::{Context, anyhow};
use minotari_node_grpc_client::{BaseNodeGrpcClient, grpc};

use crate::process_manager::Instance;

pub struct MinoTariNodeProcess {
    instance: Instance,
}

impl MinoTariNodeProcess {
    pub fn new(instance: Instance) -> Self {
        Self { instance }
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn instance_mut(&mut self) -> &mut Instance {
        &mut self.instance
    }

    async fn connect_client(&self) -> anyhow::Result<BaseNodeGrpcClient<tonic::transport::Channel>> {
        let port = self
            .instance
            .allocated_ports()
            .get("grpc")
            .ok_or_else(|| anyhow!("No grpc port allocated"))?;
        let client = BaseNodeGrpcClient::connect(format!("http://localhost:{}", port)).await?;
        Ok(client)
    }

    pub fn get_identity(&self) -> anyhow::Result<String> {
        // We cannot call identify because we'd need to override the allowed methods via cli, and this is not
        // supported. So we read from the base node identity file
        let id_file = self.instance.base_path().join("config").join("base_node_id.json");
        let mut config =
            File::open(&id_file).with_context(|| format!("Loading base node ID failed {}", id_file.display()))?;
        let identity = serde_json5::from_reader::<_, serde_json::Value>(&mut config)?;
        let public_key = identity["public_key"]
            .as_str()
            .ok_or_else(|| anyhow!("public_key not found or not a string"))?;
        let public_addresses = identity["public_addresses"]
            .as_array()
            .ok_or_else(|| anyhow!("public_addresses not found or not an array"))?;
        let public_addresses = public_addresses
            .iter()
            .map(|v| v.as_str().ok_or_else(|| anyhow!("public_address not a string")))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(format!("{}::{}", public_key, public_addresses.join(",")))
    }

    pub async fn get_chain_metadata(&self) -> anyhow::Result<Option<grpc::MetaData>> {
        let mut client = self.connect_client().await?;
        let resp = client.get_tip_info(grpc::Empty {}).await?;
        let resp = resp.into_inner();
        Ok(resp.metadata)
    }
}
