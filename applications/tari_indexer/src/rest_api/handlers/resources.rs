//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{extract::Path, Extension, Json};
use tari_indexer_client::types::GetResourceResponse;
use tari_template_lib::{constants::XTR, models::ResourceAddress};

use crate::{
    rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult},
    substate_manager::SubstateResponse,
};

#[utoipa::path(
    get,
    path = "/resources/{resource_address}",
    description = "Fetches a resource by ID"
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

    let is_xtr = resource_address == XTR;

    let SubstateResponse { substate, version, .. } = context
        .substate_manager()
        .get_substate(&resource_address.into(), None)
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Error getting resource: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Resource {} not found", resource_address)))?;

    let resource = substate.into_resource().ok_or_else(|| {
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

pub async fn get_xtr(context: Extension<HandlerContext>) -> HandlerResult<Json<GetResourceResponse>> {
    get_resource(context, Path(XTR)).await
}
