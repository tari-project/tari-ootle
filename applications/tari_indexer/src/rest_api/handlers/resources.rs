//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::iter;

use axum::{Extension, Json, extract::Path};
use tari_engine_types::substate::SubstateId;
use tari_indexer_client::types::GetResourceResponse;
use tari_ootle_common_types::SubstateRequirementRef;
use tari_template_lib_types::{ResourceAddress, constants::TARI_TOKEN};

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

#[utoipa::path(
    get,
    path = "/resources/{resource_address}",
    description = "Fetches a resource by ID",
    responses(
        (status = 200, description = "Resource details", body = GetResourceResponse),
        (status = 404, description = "Resource not found", body = ErrorResponse),
        (status = SERVICE_UNAVAILABLE, description = "Indexer is still syncing", body = ErrorResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to fetch resource", body = ErrorResponse),
    ),
)]
pub async fn get_resource(
    Extension(context): Extension<HandlerContext>,
    Path(resource_address): Path<ResourceAddress>,
) -> HandlerResult<Json<GetResourceResponse>> {
    if !context
        .epoch_manager()
        .is_initial_scanning_complete()
        .await
        .map_err(ErrorResponse::anyhow)?
    {
        return Err(ErrorResponse::service_unavailable(
            "Indexer is still syncing. Please try again later.",
        ));
    }

    let is_xtr = resource_address == TARI_TOKEN;
    let resource_address = SubstateId::Resource(resource_address);

    let (_, fetched) = context
        .substate_manager()
        .get_substates(iter::once(SubstateRequirementRef::new(&resource_address, None)))
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Error getting resource: {}", e)))?
        .into_iter()
        .next()
        .ok_or_else(|| ErrorResponse::not_found(format!("Resource {} not found", resource_address)))?;

    let substate = fetched.substate;
    let version = substate.version();
    let resource = substate.into_substate_value().into_resource().ok_or_else(|| {
        ErrorResponse::general_error(format!(
            "Substate at address {} is not a resource. This indicates a data inconsistency.",
            resource_address
        ))
    })?;

    let total_supply = if is_xtr {
        // XTR total supply is handled differently.
        let amount = context
            .read_only_store()
            .get_xtr_total_supply()
            .await
            .map_err(ErrorResponse::anyhow)?;
        Some(amount)
    } else {
        resource.total_supply()
    };

    Ok(Json(GetResourceResponse {
        resource,
        version,
        total_supply,
    }))
}

#[utoipa::path(
    get,
    path = "/resources/tari",
    description = "Fetches the TARI resource",
    responses(
        (status = 200, description = "Resource details", body = GetResourceResponse),
        (status = 404, description = "Resource not found", body = ErrorResponse),
        (status = SERVICE_UNAVAILABLE, description = "Indexer is still syncing", body = ErrorResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to fetch resource", body = ErrorResponse),
    ),
)]
pub async fn get_tari(context: Extension<HandlerContext>) -> HandlerResult<Json<GetResourceResponse>> {
    get_resource(context, Path(TARI_TOKEN)).await
}
