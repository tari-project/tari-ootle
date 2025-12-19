//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{extract::Query, Extension, Json};
use tari_indexer_client::types::{GetNonFungiblesRequest, GetNonFungiblesResponse};

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

#[utoipa::path(post, path = "/non-fungibles", description = "Get non-fungibles by resource address")]
pub async fn get_non_fungibles(
    Extension(context): Extension<HandlerContext>,
    Query(req): Query<GetNonFungiblesRequest>,
) -> HandlerResult<Json<GetNonFungiblesResponse>> {
    if req.end_index < req.start_index {
        return Err(ErrorResponse::bad_request(format!(
            "end_index ({}) must be greater than or equal to start_index ({})",
            req.end_index, req.start_index
        )));
    }
    let limit = usize::try_from(req.end_index.saturating_sub(req.start_index))
        .map_err(|e| ErrorResponse::bad_request(format!("Invalid end_index: {}", e)))?;
    let offset = usize::try_from(req.start_index)
        .map_err(|e| ErrorResponse::bad_request(format!("Invalid start_index: {}", e)))?;
    if limit > 1000 {
        return Err(ErrorResponse::bad_request(
            "The maximum allowed limit is 1000".to_string(),
        ));
    }

    let non_fungibles = context
        .substate_manager()
        .get_non_fungibles_by_resource_address(req.address, limit, offset)
        .map_err(ErrorResponse::anyhow)?;

    Ok(Json(GetNonFungiblesResponse { non_fungibles }))
}
