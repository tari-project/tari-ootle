//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{config::InstanceType, process_manager::InstanceId, webserver::context::HandlerContext};

#[derive(Debug, Clone, Deserialize)]
pub struct ListValidatorNodesRequest {}

#[derive(Debug, Clone, Serialize)]
pub struct ListValidatorNodesResponse {
    pub nodes: Vec<ValidatorNodeInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidatorNodeInfo {
    pub instance_id: InstanceId,
    pub name: String,
    pub web: Url,
    pub jrpc: Url,
    pub is_running: bool,
}

pub async fn list(
    context: &HandlerContext,
    _req: ListValidatorNodesRequest,
) -> Result<ListValidatorNodesResponse, anyhow::Error> {
    let instances = context.process_manager().list_validator_nodes().await?;

    let nodes = instances
        .into_iter()
        .map(|instance| {
            let jrpc = instance.get_public_json_rpc_url();
            let web = instance.get_public_web_url();

            Ok(ValidatorNodeInfo {
                instance_id: instance.id,
                name: instance.name,
                web,
                jrpc,
                is_running: instance.is_running,
            })
        })
        .collect::<anyhow::Result<_>>()?;

    Ok(ListValidatorNodesResponse { nodes })
}

#[derive(Debug, Clone, Deserialize)]
pub struct ValidatorNodeCreateRequest {
    name: Option<String>,
    register: bool,
    mine: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidatorNodeCreateResponse {
    pub instance_id: InstanceId,
}

pub async fn create(
    context: &HandlerContext,
    req: ValidatorNodeCreateRequest,
) -> Result<ValidatorNodeCreateResponse, anyhow::Error> {
    let process_manager = context.process_manager();

    let name = match req.name {
        Some(name) => name,
        None => {
            let all_instances = process_manager.list_instances(None).await?;
            format!("New VN-#{}", all_instances.len())
        },
    };
    let instance_id = process_manager
        .create_instance(name, InstanceType::TariValidatorNode, HashMap::new())
        .await?;

    if req.register {
        process_manager.register_validator_node(instance_id).await?;
        if req.mine {
            process_manager.mine_blocks(10).await?;
        }
    }

    Ok(ValidatorNodeCreateResponse { instance_id })
}
