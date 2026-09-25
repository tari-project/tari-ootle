//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use tari_ootle_walletd_client::{
    permissions::{Permission, ReadOnly},
    types::{ResourcesGetManyRequest, ResourcesGetManyResponse, ResourcesGetRequest, ResourcesGetResponse},
};

use crate::handlers::{
    HandlerContext,
    helpers::{OrJrpcNotFound, invalid_params},
};

/// Largest number of addresses `resources.get_many` looks up in one request. Each address is a
/// parameter of a single SQL `IN` query.
const MAX_RESOURCES_PER_REQUEST: usize = 500;

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: ResourcesGetRequest,
) -> Result<ResourcesGetResponse, anyhow::Error> {
    context.authorize(token, &[Permission::Resources(ReadOnly::Read)])?;
    let resource = context
        .wallet_sdk()
        .resources_api()
        .get(&req.address)
        .or_jrpc_not_found()?;
    Ok(ResourcesGetResponse { resource })
}

pub async fn handle_get_many(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: ResourcesGetManyRequest,
) -> Result<ResourcesGetManyResponse, anyhow::Error> {
    context.authorize(token, &[Permission::Resources(ReadOnly::Read)])?;
    if req.addresses.len() > MAX_RESOURCES_PER_REQUEST {
        return Err(invalid_params(
            "addresses",
            Some(format!(
                "{} addresses given; at most {MAX_RESOURCES_PER_REQUEST} can be fetched in one request",
                req.addresses.len()
            )),
        ));
    }
    let resources = context.wallet_sdk().resources_api().get_many(&req.addresses)?;
    Ok(ResourcesGetManyResponse { resources })
}
