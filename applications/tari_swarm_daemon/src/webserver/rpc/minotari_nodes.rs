//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    config::InstanceType,
    process_manager::InstanceId,
    webserver::{context::HandlerContext, rpc::instances::InstanceInfo},
};

#[derive(Debug, Clone, Deserialize)]
pub struct BaseNodeCreateRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaseNodeCreateResponse {
    pub instance_id: InstanceId,
}

pub async fn create(
    context: &HandlerContext,
    req: BaseNodeCreateRequest,
) -> Result<BaseNodeCreateResponse, anyhow::Error> {
    let instance_id = context
        .process_manager()
        .create_instance(req.name, InstanceType::MinoTariNode, HashMap::new())
        .await?;

    Ok(BaseNodeCreateResponse { instance_id })
}

#[derive(Debug, Clone, Deserialize)]
pub struct BaseNodeGetRequest {
    pub instance_id: InstanceId,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaseNodeGetResponse {
    pub instance_info: InstanceInfo,
    pub height: Option<u64>,
}

pub async fn get(context: &HandlerContext, req: BaseNodeGetRequest) -> Result<BaseNodeGetResponse, anyhow::Error> {
    let details = context
        .process_manager()
        .get_minotari_node_details(req.instance_id)
        .await?;
    Ok(BaseNodeGetResponse {
        instance_info: details.instance_info.into(),
        height: details.height,
    })
}
