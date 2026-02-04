//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{
    Extension,
    Json,
    response::{IntoResponse, Response},
};
use serde_json::json;
use tari_epoch_manager::EpochManagerReader;
use tari_epoch_oracles::store::StoreKey;
use tari_indexer_client::{
    rest_api::types::GetIdentityResponse,
    types::{GetEpochManagerStatsResponse, IndexerReadyResponse},
};

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

#[utoipa::path(
    get,
    path = "/identity",
    description = "Get indexer network identity information",
    responses(
        (status = 200, description = "Indexer network identity info", body = GetIdentityResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to get identity info", body = ErrorResponse),
    ),
)]
pub async fn get_identity(Extension(context): Extension<HandlerContext>) -> HandlerResult<Response> {
    let info = context
        .networking()
        .get_local_peer_info()
        .await
        .map_err(ErrorResponse::anyhow)?;
    let response = GetIdentityResponse {
        peer_id: info.peer_id.to_string(),
        public_key: *context.public_key(),
        public_addresses: info.listen_addrs,
    };
    Ok(context.apply_cache_control(Json(response), 1000))
}

#[utoipa::path(
    get,
    path = "/wait-until-ready",
    description = "Request will return an empty result when the indexer is ready to serve requests. This may take an \
                   indefinite amount of time after startup."
)]
pub async fn wait_until_ready(
    Extension(context): Extension<HandlerContext>,
) -> HandlerResult<Json<IndexerReadyResponse>> {
    // TODO: we should rather return the current state of the indexer and have clients poll
    context
        .epoch_manager()
        .wait_for_initial_scanning_to_complete()
        .await
        .map_err(ErrorResponse::anyhow)?;

    Ok(Json(IndexerReadyResponse {}))
}

#[utoipa::path(get, path = "/epoch-manager/stats", description = "Get epoch manager stats")]
pub async fn get_epoch_manager_stats(
    Extension(context): Extension<HandlerContext>,
) -> HandlerResult<Json<GetEpochManagerStatsResponse>> {
    let current_epoch = context.epoch_manager().get_current_epoch();
    let current_epoch_hash = context
        .epoch_manager()
        .get_current_epoch_hash()
        .await
        .map_err(ErrorResponse::anyhow)?;

    let current_block_height = context
        .global_db()
        .create_transaction()
        .and_then(|mut tx| {
            context
                .global_db()
                .metadata(&mut tx)
                .get_metadata::<u64>(StoreKey::BaseLayerLastScannedBlockHeight.as_key_bytes())
        })
        .map_err(ErrorResponse::anyhow)?;

    let response = GetEpochManagerStatsResponse {
        current_epoch,
        current_block_height: current_block_height.unwrap_or(0),
        current_block_hash: current_epoch_hash,
    };
    Ok(Json(response))
}

#[utoipa::path(get, path = "/health", description = "Health check")]
pub async fn health(Extension(context): Extension<HandlerContext>) -> impl IntoResponse {
    let current_epoch = context.epoch_manager().get_current_epoch();
    let epoch_manager_ok = !context.epoch_manager().is_closed();
    let network_ok = !context.networking().is_closed();
    let db_ok = context
        .global_db()
        .with_read_tx(|tx| tx.execute_sql("SELECT 1"))
        .is_ok();
    let status = if epoch_manager_ok && network_ok && db_ok {
        "ok"
    } else {
        "error"
    };

    let is_ok = epoch_manager_ok && network_ok && db_ok;

    let body = json!({
        "status": status,
        "current_epoch": current_epoch.as_u64(),
        "epoch_manager_ok": epoch_manager_ok,
        "network_ok": network_ok,
        "db_ok": db_ok,
    });

    if is_ok {
        (axum::http::StatusCode::OK, Json(body)).into_response()
    } else {
        (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response()
    }
}

#[utoipa::path(get, path = "/ready", description = "Indexer readiness check")]
pub async fn ready(Extension(context): Extension<HandlerContext>) -> HandlerResult<Json<serde_json::Value>> {
    match context.epoch_manager().is_initial_scanning_complete().await {
        Ok(true) => Ok(Json(json!({}))),
        Ok(false) => Err(ErrorResponse::service_unavailable(
            "Indexer is still scanning".to_string(),
        )),
        Err(e) => Err(ErrorResponse::anyhow(e)),
    }
}
