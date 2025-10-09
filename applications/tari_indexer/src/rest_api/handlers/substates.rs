//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use axum::{
    extract::{Path, Query},
    response::Response,
    Extension,
    Json,
};
use tari_engine_types::substate::SubstateId;
use tari_indexer_client::types::{
    GetSubstateRequest,
    GetSubstateResponse,
    GetSubstatesRequest,
    GetSubstatesResponse,
    ListSubstatesRequest,
    ListSubstatesResponse,
};
use tari_ootle_common_types::SubstateRequirementRef;
use tari_validator_node_rpc::client::SubstateResult;

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

#[utoipa::path(
    get,
    path = "/substates",
    description = "List substates, optionally filtered by type or template",
    params(
        ("filter_by_type" = Option<String>, Query, description = "Filter substates by type"),
        ("filter_by_template" = Option<String>, Query, description = "Filter substates by template address"),
        ("limit" = Option<u32>, Query, description = "Limit the number of results returned"),
        ("offset" = Option<u32>, Query, description = "Offset the results by this amount"),
    ),
)]
pub async fn list_substates(
    Extension(context): Extension<HandlerContext>,
    Query(req): Query<ListSubstatesRequest>,
) -> HandlerResult<Response> {
    let ListSubstatesRequest {
        filter_by_template,
        filter_by_type,
        limit,
        offset,
    } = req;

    let substates = context
        .substate_manager()
        .get_stored_substates_by_filters(filter_by_type, filter_by_template, limit, offset)
        .map_err(|e| ErrorResponse::anyhow(anyhow!("Error getting substate: {}", e)))?;

    Ok(context.apply_cache_control(Json(ListSubstatesResponse { substates }), 100))
}

#[utoipa::path(
    get,
    path = "/substates/{substate_id}",
    description = "Fetches a substate by ID",
    params(
        ("substate_id" = String, Path, description = "The substate ID to fetch"),
        ("local_search_only" = bool, Query, description = "If true, only search local storage for the substate"),
        ("version" = Option<u32>, Query, description = "Minimum version of the substate to fetch"),
    ),
)]
pub async fn get_substate(
    Extension(context): Extension<HandlerContext>,
    Path(substate_id): Path<SubstateId>,
    Query(req): Query<GetSubstateRequest>,
) -> HandlerResult<Json<GetSubstateResponse>> {
    let maybe_substate = context
        .substate_manager()
        .get_substate(&substate_id, req.version)
        .await
        .map_err(|e| ErrorResponse::internal_error(format!("Error getting substate: {}", e)))?;

    match maybe_substate {
        Some(substate_resp) => Ok(Json(GetSubstateResponse {
            version: substate_resp.version,
            substate: substate_resp.substate,
        })),
        None => {
            if req.local_search_only {
                Err(ErrorResponse::not_found(format!("Substate {} not found", substate_id)))
            } else {
                // Ask network
                let substate = context
                    .transaction_manager()
                    .get_substate_from_network(SubstateRequirementRef::new(&substate_id, req.version))
                    .await
                    .map_err(ErrorResponse::anyhow)?;
                match substate {
                    SubstateResult::DoesNotExist => Err(ErrorResponse::not_found(format!(
                        "Substate {} not found on network",
                        substate_id
                    ))),
                    SubstateResult::Up { substate, .. } => Ok(Json(GetSubstateResponse {
                        version: substate.version(),
                        substate: substate.into_substate_value(),
                    })),
                    SubstateResult::Down { version, .. } => Err(ErrorResponse::not_found(format!(
                        "Substate {}v{} is DOWN on network",
                        substate_id, version
                    ))),
                }
            }
        },
    }
}

#[utoipa::path(
    post,
    path = "/substates/fetch",
    description = "Fetches several substates by their IDs"
)]
pub async fn fetch_substates(
    Extension(context): Extension<HandlerContext>,
    Json(req): Json<GetSubstatesRequest>,
) -> HandlerResult<Json<GetSubstatesResponse>> {
    const MAX_REQUESTS: usize = 20;

    let GetSubstatesRequest { requests } = req;

    if requests.len() > MAX_REQUESTS {
        return Err(ErrorResponse::bad_request(format!(
            "Cannot request more than {MAX_REQUESTS} substates at once"
        )));
    }

    let substates = context
        .substate_manager()
        .get_substates(requests.as_slice())
        .map_err(|e| ErrorResponse::internal_error(format!("Error getting substate: {}", e)))?;

    Ok(Json(GetSubstatesResponse { substates }))
}
