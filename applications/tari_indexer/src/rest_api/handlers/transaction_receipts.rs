//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{
    extract::{Path, Query},
    response::Response,
    Extension,
    Json,
};
use tari_engine_types::transaction_receipt::TransactionReceiptAddress;
use tari_indexer_client::types::{
    GetTransactionReceiptResponse,
    ListTransactionReceiptsRequest,
    ListTransactionReceiptsResponse,
};
use tari_ootle_common_types::optional::Optional;

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

#[utoipa::path(get, path = "/transaction-receipts", description = "List transaction receipts")]
pub async fn list_transaction_receipts(
    Extension(context): Extension<HandlerContext>,
    Query(req): Query<ListTransactionReceiptsRequest>,
) -> HandlerResult<Response> {
    let limit = req.limit.unwrap_or(100);
    if limit > 100 {
        return Err(ErrorResponse::bad_request(
            "Limit cannot be greater than 100".to_string(),
        ));
    }

    let receipts = context
        .read_only_store()
        .list_transaction_receipts(req.last_id, u64::from(limit), req.ordering)
        .map_err(ErrorResponse::anyhow)?;

    Ok(context.apply_cache_control(Json(ListTransactionReceiptsResponse { receipts }), 10))
}

#[utoipa::path(
    get,
    path = "/transaction-receipt/{transaction_id}",
    description = "Get the transaction receipt by transaction ID"
)]
pub async fn get_transaction_receipt(
    Extension(context): Extension<HandlerContext>,
    Path(receipt_addr): Path<TransactionReceiptAddress>,
) -> HandlerResult<Json<GetTransactionReceiptResponse>> {
    let receipt = context
        .read_only_store()
        .get_transaction_receipt(&receipt_addr)
        .optional()
        .map_err(ErrorResponse::anyhow)?
        .ok_or_else(|| ErrorResponse::not_found(format!("Transaction receipt {receipt_addr} not found")))?;

    Ok(Json(GetTransactionReceiptResponse { receipt }))
}
