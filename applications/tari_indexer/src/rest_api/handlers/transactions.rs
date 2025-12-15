//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{
    extract::{Path, Query},
    response::Response,
    Extension,
    Json,
};
use log::*;
use tari_consensus_types::Decision;
use tari_indexer_client::types::{
    GetTransactionResultResponse,
    IndexerTransactionFinalizedResult,
    ListRecentTransactionsRequest,
    ListRecentTransactionsResponse,
    SubmitTransactionRequest,
    SubmitTransactionResponse,
};
use tari_ootle_common_types::optional::Optional;
use tari_rpc_framework::RpcStatusCode;
use tari_transaction::TransactionId;
use tari_validator_node_rpc::client::TransactionResultStatus;
use time::{OffsetDateTime, PrimitiveDateTime};

use crate::{
    network_client::NetworkClientError,
    rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult},
    transaction_manager::error::TransactionManagerError,
};

const LOG_TARGET: &str = "tari::indexer::rest_api::handlers::transactions";

#[utoipa::path(
    post,
    path = "/transactions",
    description = "Submit a transaction to validators responsible for the involved shards"
)]
pub async fn submit_transaction(
    Extension(context): Extension<HandlerContext>,
    Json(req): Json<SubmitTransactionRequest>,
) -> HandlerResult<Json<SubmitTransactionResponse>> {
    let request: SubmitTransactionRequest = req;

    if request.transaction.is_dry_run() {
        return Err(ErrorResponse::bad_request(
            "Dry-run transactions must be submitted to the /transactions/dry-run endpoint".to_string(),
        ));
    }

    let transaction = request.transaction;
    let transaction_id = context
        .transaction_manager()
        .submit_transaction(transaction)
        .await
        .map_err(|e| match e {
            TransactionManagerError::NetworkClientError(NetworkClientError::AllValidatorsFailed {
                last_error: Some(last_err),
                committee_size,
            }) => match last_err.status() {
                Some(status) => match status.as_status_code() {
                    RpcStatusCode::BadRequest => {
                        ErrorResponse::bad_request(format!("Bad request: {}", status.details()))
                    },
                    RpcStatusCode::NotFound => ErrorResponse::not_found(format!("Not found: {}", status.details())),
                    _ => ErrorResponse::general_error(format!(
                        "Rpc error: ({} members) {}",
                        committee_size,
                        status.details()
                    )),
                },
                None => ErrorResponse::general_error(format!("Rpc error: ({} members) {}", committee_size, last_err)),
            },
            TransactionManagerError::NetworkClientError(NetworkClientError::AllValidatorsFailed { .. }) |
            TransactionManagerError::NetworkClientError(NetworkClientError::NoCommitteeMembers) => {
                ErrorResponse::service_unavailable(format!("All validators failed: {}", e))
            },
            TransactionManagerError::InvalidTransaction {
                transaction_id,
                details,
            } => ErrorResponse::bad_request(format!("Transaction {} is invalid: {}", transaction_id, details)),
            e => ErrorResponse::anyhow(e),
        })?;

    info!(target: LOG_TARGET, "✅ Transaction submitted: {}", transaction_id);

    Ok(Json(SubmitTransactionResponse {
        result: IndexerTransactionFinalizedResult::Pending,
        transaction_id,
    }))
}

#[utoipa::path(
    post,
    path = "/transactions/dry-run",
    description = "Submit a transaction as a dry-run"
)]
pub async fn submit_transaction_dry_run(
    Extension(context): Extension<HandlerContext>,
    Json(req): Json<SubmitTransactionRequest>,
) -> HandlerResult<Json<SubmitTransactionResponse>> {
    let request: SubmitTransactionRequest = req;

    if !request.transaction.is_dry_run() {
        return Err(ErrorResponse::bad_request(
            "Non-dry-run transactions must be submitted to the /transactions endpoint".to_string(),
        ));
    }
    let transaction_id = request.transaction.calculate_id();
    let exec_result = context
        .dry_run_transaction_processor()
        .process_transaction(request.transaction)
        .await
        .map_err(ErrorResponse::anyhow)?;

    Ok(Json(SubmitTransactionResponse {
        result: IndexerTransactionFinalizedResult::Finalized {
            execution_result: Some(Box::new(exec_result)),
            final_decision: Decision::Commit,
            abort_details: None,
            finalized_time: now(),
            execution_time: Default::default(),
        },
        transaction_id,
    }))
}

fn now() -> PrimitiveDateTime {
    let now = OffsetDateTime::now_utc();
    PrimitiveDateTime::new(now.date(), now.time())
}

#[utoipa::path(get, path = "/transactions/recent", description = "List recent transactions")]
pub async fn list_recent_transactions(
    Extension(context): Extension<HandlerContext>,
    Query(req): Query<ListRecentTransactionsRequest>,
) -> HandlerResult<Response> {
    let limit = req.limit.unwrap_or(100);
    if limit > 100 {
        return Err(ErrorResponse::bad_request(
            "Limit cannot be greater than 100".to_string(),
        ));
    }

    let transactions = context
        .transaction_manager()
        .list_recent_transactions(req.last_id, limit as usize)
        .map_err(ErrorResponse::anyhow)?;

    Ok(context.apply_cache_control(Json(ListRecentTransactionsResponse { transactions }), 30))
}

#[utoipa::path(
    get,
    path = "/transactions/{transaction_id}/result",
    description = "Get the result of a submitted transaction (by transaction ID)"
)]
pub async fn get_transaction_result(
    Extension(context): Extension<HandlerContext>,
    Path(transaction_id): Path<TransactionId>,
) -> HandlerResult<Json<GetTransactionResultResponse>> {
    let result = context
        .transaction_manager()
        .get_transaction_result(transaction_id)
        .await
        .optional()
        .map_err(ErrorResponse::anyhow)?
        .ok_or_else(|| ErrorResponse::not_found(format!("Transaction {transaction_id} not found")))?;

    let resp = match result {
        TransactionResultStatus::Pending => GetTransactionResultResponse {
            result: IndexerTransactionFinalizedResult::Pending,
        },
        TransactionResultStatus::Finalized(finalized) => GetTransactionResultResponse {
            result: IndexerTransactionFinalizedResult::Finalized {
                final_decision: finalized.final_decision,
                execution_result: finalized.execute_result.map(Box::new),
                execution_time: finalized.execution_time,
                finalized_time: finalized.finalized_time,
                abort_details: finalized.abort_details,
            },
        },
    };

    Ok(Json(GetTransactionResultResponse { result: resp.result }))
}
