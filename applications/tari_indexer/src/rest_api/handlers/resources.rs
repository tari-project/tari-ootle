//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::iter;

use axum::{extract::Path, Extension, Json};
use tari_engine_types::substate::SubstateId;
use tari_indexer_client::types::GetResourceResponse;
use tari_ootle_common_types::SubstateRequirementRef;
use tari_template_lib::{constants::XTR, models::ResourceAddress};

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

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
    let resource_address = SubstateId::Resource(resource_address);

    let (_, substate) = context
        .substate_manager()
        .get_substates(iter::once(SubstateRequirementRef::new(&resource_address, None)))
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Error getting resource: {}", e)))?
        .into_iter()
        .next()
        .ok_or_else(|| ErrorResponse::not_found(format!("Resource {} not found", resource_address)))?;

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
