//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use axum::{
    Extension,
    Json,
    extract::Query,
    http::header::HeaderMap,
    response::{IntoResponse, Response},
};
use log::*;
use tari_indexer_client::types::{
    GetUtxoUpdatesRequest,
    GetUtxosRequest,
    GetUtxosResponse,
    ListUtxosRequest,
    ListUtxosResponse,
};
use tari_ootle_common_types::NumPreshards;

use crate::rest_api::{
    context::HandlerContext,
    error::ErrorResponse,
    handlers::HandlerResult,
    streaming::{UtxoUpdateStream, encoding},
};

const LOG_TARGET: &str = "tari::ootle::indexer::rest_api::handlers::utxos";

#[utoipa::path(post, path = "/utxos/updates", description = "Streams UTXO updates")]
pub async fn stream_utxo_updates(
    headers: HeaderMap,
    Extension(context): Extension<HandlerContext>,
    Json(req): Json<GetUtxoUpdatesRequest>,
) -> HandlerResult<Response> {
    if req.per_shard_limit > 1000 {
        return Err(ErrorResponse::bad_request(
            "per_shard_limit cannot be greater than 1000",
        ));
    }

    if req.shard_state_versions.len() > NumPreshards::MAX_SHARD.as_u32() as usize {
        return Err(ErrorResponse::bad_request(format!(
            "shard_state_versions cannot contain more than {} entries",
            NumPreshards::MAX_SHARD.as_u32()
        )));
    }

    if req
        .shard_state_versions
        .iter()
        .any(|(shard, _)| shard.is_global() || *shard > NumPreshards::MAX_SHARD)
    {
        return Err(ErrorResponse::bad_request(format!(
            "shard_state_versions contains invalid shard. Cannot query UTXOs in global shard or greater than max \
             shard {} exceeded",
            NumPreshards::MAX_SHARD.as_u32()
        )));
    }

    info!(
        target: LOG_TARGET,
        "Starting UTXO update stream for {} shard(s)",
        req.shard_state_versions.len()
    );

    let accept_header = headers.get("accept").and_then(|v| v.to_str().ok());
    let encoder = match accept_header {
        Some(mime) => encoding::from_media_type(mime).ok_or_else(|| {
            ErrorResponse::bad_request(format!(
                "Unsupported Accept header '{}', supported types are 'application/x-protobuf' and 'application/json'",
                mime
            ))
        })?,
        None => encoding::MimeTypeEncoder::protobuf(),
    };

    // Check for duplicate shards
    let mut seen = HashSet::new();
    for (shard, _) in &req.shard_state_versions {
        if !seen.insert(shard) {
            return Err(ErrorResponse::bad_request(format!(
                "Duplicate shard {} in shard_state_versions",
                shard.as_u32()
            )));
        }
    }

    let stream = UtxoUpdateStream::new(context.substate_manager().clone(), req, encoder);
    Ok(stream.into_response())
}

#[utoipa::path(
    post,
    path = "/utxos/fetch",
    description = "Gets full UTXO data for a list of UTXO IDs"
)]
pub async fn fetch_utxos(
    Extension(context): Extension<HandlerContext>,
    Json(req): Json<GetUtxosRequest>,
) -> HandlerResult<Json<GetUtxosResponse>> {
    if req.tag_and_nonce_pairs.len() > 1000 {
        return Err(ErrorResponse::bad_request("cannot query more than 1000 UTXOs"));
    }
    let utxos = context
        .substate_manager()
        .get_unspent_utxos(&req.resource_address, &req.tag_and_nonce_pairs)
        .map_err(ErrorResponse::anyhow)?;

    Ok(Json(GetUtxosResponse { utxos }))
}

#[utoipa::path(get, path = "/utxos", description = "List full UTXO data")]
pub async fn list_utxos(
    Extension(context): Extension<HandlerContext>,
    Query(req): Query<ListUtxosRequest>,
) -> HandlerResult<Json<ListUtxosResponse>> {
    if req.limit == 0 {
        return Err(ErrorResponse::bad_request("limit must be greater than 0"));
    }
    if req.limit > 1000 {
        return Err(ErrorResponse::bad_request("cannot query more than 1000 UTXOs"));
    }
    let utxos = context
        .substate_manager()
        .list_utxos(&req.resource_address, req.from_id, req.limit)
        .map_err(ErrorResponse::anyhow)?;

    Ok(Json(ListUtxosResponse { utxos }))
}
