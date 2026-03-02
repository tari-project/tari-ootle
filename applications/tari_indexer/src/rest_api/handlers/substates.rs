//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{array, iter};

use axum::{
    Extension,
    Json,
    extract::{Path, Query},
};
use tari_engine_types::substate::SubstateId;
use tari_indexer_client::types::{GetSubstateRequest, GetSubstateResponse, GetSubstatesRequest, GetSubstatesResponse};
use tari_ootle_common_types::SubstateRequirementRef;

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

#[utoipa::path(
    get,
    path = "/substates/{substate_id}",
    description = "Fetches a substate by ID",
    params(
        ("substate_id" = String, Path, description = "The substate ID to fetch"),
        ("local_search_only" = bool, Query, description = "If true, only search local storage for the substate"),
        ("version" = Option<u32>, Query, description = "Minimum version of the substate to fetch"),
    ),
    responses(
        (status = 200, description = "Substate details", body = GetSubstateResponse),
        (status = 404, description = "Substate not found", body = ErrorResponse),
        (status = SERVICE_UNAVAILABLE, description = "Indexer is still syncing", body = ErrorResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to fetch substate", body = ErrorResponse),
    )
)]
pub async fn get_substate(
    Extension(context): Extension<HandlerContext>,
    Path(substate_id): Path<SubstateId>,
    Query(req): Query<GetSubstateRequest>,
) -> HandlerResult<Json<GetSubstateResponse>> {
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
    let requirement = SubstateRequirementRef::new(&substate_id, req.version);

    let manager = context.substate_manager();
    let maybe_substate = if req.local_search_only {
        manager
            .get_cached_substates(array::from_ref(requirement.substate_id()))
            .await
            .map(|a| {
                a.into_iter()
                    .find(|(_, substate)| req.version.is_none_or(|v| substate.version() == v))
            })
            .map_err(|e| ErrorResponse::internal_error(format!("Error getting substate: {}", e)))?
    } else {
        manager
            .get_substates(iter::once(requirement))
            .await
            .map(|a| a.into_iter().next())
            .map_err(|e| ErrorResponse::internal_error(format!("Error getting substate: {}", e)))?
    };

    match maybe_substate {
        Some((_, substate)) => Ok(Json(GetSubstateResponse {
            version: substate.version(),
            substate: substate.into_substate_value(),
        })),
        None => Err(ErrorResponse::not_found(format!("Substate {} not found", substate_id))),
    }
}

#[utoipa::path(
    post,
    path = "/substates/fetch",
    description = "Fetches several substates by their IDs",
    responses(
        (status = 200, description = "Substates details", body = GetSubstatesResponse),
        (status = BAD_REQUEST, description = "Too many substates requested", body = ErrorResponse),
        (status = SERVICE_UNAVAILABLE, description = "Indexer is still syncing", body = ErrorResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to fetch substates", body = ErrorResponse),
    ),
)]
pub async fn fetch_substates(
    Extension(context): Extension<HandlerContext>,
    Json(req): Json<GetSubstatesRequest>,
) -> HandlerResult<Json<GetSubstatesResponse>> {
    const MAX_REQUESTS: usize = 20;

    let GetSubstatesRequest { requests, cached_only } = req;

    if requests.len() > MAX_REQUESTS {
        return Err(ErrorResponse::bad_request(format!(
            "Cannot request more than {MAX_REQUESTS} substates at once"
        )));
    }

    if cached_only {
        let substates = context
            .substate_manager()
            .get_cached_substates(requests.as_slice())
            .await
            .map_err(|e| ErrorResponse::internal_error(format!("Error getting substate: {}", e)))?;

        return Ok(Json(GetSubstatesResponse { substates }));
    }

    let substates = context
        .substate_manager()
        .fetch_and_cache_substates(requests.as_slice())
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Error getting substates: {}", e)))?;

    Ok(Json(GetSubstatesResponse { substates }))
}
