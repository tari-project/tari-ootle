//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{Extension, Json};
use tari_indexer_client::types::{
    GetLatestEpochCheckpointResponse,
    ListEpochCheckpointsRequest,
    ListEpochCheckpointsResponse,
};
use tari_ootle_common_types::Epoch;

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 100;

#[utoipa::path(
    get,
    path = "/epoch-checkpoints",
    description = "List epoch checkpoints from storage",
    params(
        ("from_epoch" = Option<u64>, Query, description = "Epoch to start listing from (inclusive). Defaults to 0"),
        ("limit" = Option<u32>, Query, description = "Maximum number of checkpoints to return (default: 20, max: 100)"),
    ),
    responses(
        (status = 200, description = "List of epoch checkpoints", body = ListEpochCheckpointsResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Internal error", body = ErrorResponse),
    )
)]
pub async fn list_epoch_checkpoints(
    Extension(context): Extension<HandlerContext>,
    axum::extract::Query(req): axum::extract::Query<ListEpochCheckpointsRequest>,
) -> HandlerResult<Json<ListEpochCheckpointsResponse>> {
    let from_epoch = req.from_epoch.unwrap_or(Epoch(0));
    let limit = req.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);

    let checkpoints = context
        .read_only_store()
        .epoch_checkpoint_get_all(from_epoch, u64::from(limit))
        .map_err(ErrorResponse::anyhow)?;

    let checkpoints = checkpoints
        .into_iter()
        .map(|cp| serde_json::to_value(cp).map_err(ErrorResponse::anyhow))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(ListEpochCheckpointsResponse { checkpoints }))
}

#[utoipa::path(
    get,
    path = "/epoch-checkpoints/latest",
    description = "Get the latest epoch checkpoint",
    responses(
        (status = 200, description = "The latest epoch checkpoint", body = GetLatestEpochCheckpointResponse),
        (status = 404, description = "No epoch checkpoints found", body = ErrorResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Internal error", body = ErrorResponse),
    )
)]
pub async fn get_latest_epoch_checkpoint(
    Extension(context): Extension<HandlerContext>,
) -> HandlerResult<Json<GetLatestEpochCheckpointResponse>> {
    let checkpoint = context
        .read_only_store()
        .epoch_checkpoint_get_latest()
        .map_err(ErrorResponse::anyhow)?;

    let checkpoint = serde_json::to_value(checkpoint).map_err(ErrorResponse::anyhow)?;

    Ok(Json(GetLatestEpochCheckpointResponse { checkpoint }))
}
