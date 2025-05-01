//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{DefaultBodyLimit, Extension},
    headers,
    headers::authorization::Bearer,
    routing::post,
    Router,
    TypedHeader,
};
use axum_jrpc::{
    error::{JsonRpcError, JsonRpcErrorReason},
    JrpcResult,
    JsonRpcAnswer,
    JsonRpcExtractor,
    JsonRpcResponse,
};
use log::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use tokio::task;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use super::handlers::{substates, templates, webauthn, HandlerContext};
use crate::handlers::{
    accounts,
    auth::jwt::JwtApiError,
    confidential,
    error::HandlerError,
    keys,
    nfts,
    rpc,
    settings,
    transaction,
    validator,
    webrtc,
    Handler,
};

const LOG_TARGET: &str = "tari::dan::wallet_daemon::json_rpc";

pub fn spawn_listener(
    preferred_address: SocketAddr,
    signaling_server_address: SocketAddr,
    context: HandlerContext,
) -> anyhow::Result<(SocketAddr, task::JoinHandle<anyhow::Result<()>>)> {
    let shutdown_signal = context.shutdown_signal().clone();
    let router = Router::new()
        .route("/", post(handler))
        .route("/json_rpc", post(handler))
        // TODO: Get these traces to work
        .layer(TraceLayer::new_for_http())
        .layer(Extension(Arc::new(context)))
        .layer(Extension((preferred_address,signaling_server_address)))
        .layer(CorsLayer::permissive())
        // Limit the body size to 5MB to allow for wasm uploads
        .layer(DefaultBodyLimit::max(5*1024*1024));

    let server = axum::Server::try_bind(&preferred_address)?;
    let server = server.serve(router.into_make_service());
    let listen_addr = server.local_addr();
    info!(target: LOG_TARGET, "🌐 JSON-RPC listening on {listen_addr}");
    let server = server.with_graceful_shutdown(shutdown_signal);
    let task = tokio::spawn(async move {
        server.await?;
        Ok(())
    });

    info!(target: LOG_TARGET, "💤 Stopping JSON-RPC");
    Ok((listen_addr, task))
}

#[allow(clippy::too_many_lines)]
async fn handler(
    Extension(context): Extension<Arc<HandlerContext>>,
    Extension(addresses): Extension<(SocketAddr, SocketAddr)>,
    authorization_header: Option<TypedHeader<headers::Authorization<Bearer>>>,
    value: JsonRpcExtractor,
) -> JrpcResult {
    let token = authorization_header.map(|auth| auth.0 .0);
    info!(target: LOG_TARGET, "🌐 JSON-RPC request: {}", value.method);
    debug!(target: LOG_TARGET, "🌐 JSON-RPC request: {:?}", value);
    match value.method.as_str().split_once('.') {
        Some(("auth", method)) => match method {
            "request" => call_handler(context, value, token, rpc::handle_login_request).await,
            "accept" => call_handler(context, value, token, rpc::handle_login_accept).await,
            "deny" => call_handler(context, value, token, rpc::handle_login_deny).await,
            "revoke" => call_handler(context, value, token, rpc::handle_revoke).await,
            "get_all_jwt" => call_handler(context, value, token, rpc::handle_get_all_jwt).await,
            "method" => call_handler(context, value, token, rpc::handle_get_auth_method).await,
            _ => Ok(value.method_not_found(&value.method)),
        },
        Some(("webauthn", method)) => match method {
            "already_registered" => call_handler(context, value, token, webauthn::handle_already_registered).await,
            "reg_start" => call_handler(context, value, token, webauthn::handle_start_registration).await,
            "reg_finish" => call_handler(context, value, token, webauthn::handle_finish_registration).await,
            "auth_start" => call_handler(context, value, token, webauthn::handle_start_auth).await,
            _ => Ok(value.method_not_found(&value.method)),
        },
        Some(("settings", method)) => match method {
            "get" => call_handler(context, value, token, settings::handle_get).await,
            "set" => call_handler(context, value, token, settings::handle_set).await,
            _ => Ok(value.method_not_found(&value.method)),
        },
        Some(("webrtc", "start")) => webrtc::handle_start(context, value, token.as_ref(), addresses),
        Some(("rpc", "discover")) => call_handler(context, value, token, rpc::handle_discover).await,
        Some(("keys", method)) => match method {
            "create" => call_handler(context, value, token, keys::handle_create).await,
            "list" => call_handler(context, value, token, keys::handle_list).await,
            "set_active" => call_handler(context, value, token, keys::handle_set_active).await,
            _ => Ok(value.method_not_found(&value.method)),
        },
        Some(("transactions", method)) => match method {
            "submit_instruction" => call_handler(context, value, token, transaction::handle_submit_instruction).await,
            "submit" => call_handler(context, value, token, transaction::handle_submit).await,
            "submit_dry_run" => call_handler(context, value, token, transaction::handle_submit_dry_run).await,
            "submit_manifest" => call_handler(context, value, token, transaction::handle_submit_manifest).await,
            "publish_template" => call_handler(context, value, token, transaction::handle_publish_template).await,
            "get" => call_handler(context, value, token, transaction::handle_get).await,
            "get_result" => call_handler(context, value, token, transaction::handle_get_result).await,
            "wait_result" => call_handler(context, value, token, transaction::handle_wait_result).await,
            "list" | "get_all" => call_handler(context, value, token, transaction::handle_get_all).await,
            _ => Ok(value.method_not_found(&value.method)),
        },
        Some(("accounts", method)) => match method {
            "reveal_funds" => call_handler(context, value, token, accounts::handle_reveal_funds).await,
            "claim_burn" => call_handler(context, value, token, accounts::handle_claim_burn).await,
            "create" => call_handler(context, value, token, accounts::handle_create).await,
            "list" => call_handler(context, value, token, accounts::handle_list).await,
            "get_balances" => call_handler(context, value, token, accounts::handle_get_balances).await,
            "invoke" => call_handler(context, value, token, accounts::handle_invoke).await,
            "get" => call_handler(context, value, token, accounts::handle_get).await,
            "get_default" => call_handler(context, value, token, accounts::handle_get_default).await,
            "transfer" => call_handler(context, value, token, accounts::handle_transfer).await,
            "confidential_transfer" => {
                call_handler(context, value, token, accounts::handle_confidential_transfer).await
            },
            "set_default" => call_handler(context, value, token, accounts::handle_set_default).await,
            "create_free_test_coins" => {
                call_handler(context, value, token, accounts::handle_create_free_test_coins).await
            },
            _ => Ok(value.method_not_found(&value.method)),
        },
        Some(("confidential", method)) => match method {
            "create_transfer_proof" => {
                call_handler(context, value, token, confidential::handle_create_transfer_proof).await
            },
            "finalize" => call_handler(context, value, token, confidential::handle_finalize_transfer).await,
            "cancel" => call_handler(context, value, token, confidential::handle_cancel_transfer).await,
            "create_output_proof" => {
                call_handler(context, value, token, confidential::handle_create_output_proof).await
            },
            "view_vault_balance" => call_handler(context, value, token, confidential::handle_view_vault_balance).await,
            _ => Ok(value.method_not_found(&value.method)),
        },
        Some(("substates", method)) => match method {
            "get" => call_handler(context, value, token, substates::handle_get).await,
            "list" => call_handler(context, value, token, substates::handle_list).await,
            _ => Ok(value.method_not_found(&value.method)),
        },
        Some(("templates", method)) => match method {
            "get" => call_handler(context, value, token, templates::handle_get).await,
            "list_authored" => call_handler(context, value, token, templates::handle_list_owned).await,
            _ => Ok(value.method_not_found(&value.method)),
        },
        Some(("nfts", method)) => match method {
            "mint_account_nft" => call_handler(context, value, token, nfts::handle_mint_account_nft).await,
            "get" => call_handler(context, value, token, nfts::handle_get_nft).await,
            "list" => call_handler(context, value, token, nfts::handle_list_nfts).await,
            "transfer" => call_handler(context, value, token, nfts::handle_transfer_nft).await,
            _ => Ok(value.method_not_found(&value.method)),
        },
        Some(("validators", method)) => match method {
            "get_fees" => call_handler(context, value, token, validator::handle_get_validator_fees).await,
            "claim_fees" => call_handler(context, value, token, validator::handle_claim_validator_fees).await,
            _ => Ok(value.method_not_found(&value.method)),
        },
        _ => Ok(value.method_not_found(&value.method)),
    }
}

async fn call_handler<H, TReq, TResp>(
    context: Arc<HandlerContext>,
    value: JsonRpcExtractor,
    token: Option<Bearer>,
    mut handler: H,
) -> JrpcResult
where
    TReq: DeserializeOwned,
    TResp: Serialize,
    H: for<'a> Handler<'a, TReq, Response = TResp>,
{
    let answer_id = value.get_answer_id();
    let resp = handler
        .handle(
            &context,
            token.as_ref(),
            value.parse_params().inspect_err(|e| match &e.result {
                JsonRpcAnswer::Result(_) => {
                    unreachable!("parse_params() error should not return a result")
                },
                JsonRpcAnswer::Error(e) => {
                    warn!(target: LOG_TARGET, "🌐 JSON-RPC params error: {}", e);
                },
            })?,
        )
        .await
        .map_err(|e| resolve_handler_error(answer_id, &e))?;
    Ok(JsonRpcResponse::success(answer_id, resp))
}

fn resolve_handler_error(answer_id: i64, e: &HandlerError) -> JsonRpcResponse {
    match e {
        HandlerError::Anyhow(e) => resolve_any_error(answer_id, e),
        HandlerError::NotFound => JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(
                JsonRpcErrorReason::ApplicationError(ApplicationErrorCode::NotFound as i32),
                e.to_string(),
                json!({}),
            ),
        ),
    }
}

fn resolve_any_error(answer_id: i64, e: &anyhow::Error) -> JsonRpcResponse {
    warn!(target: LOG_TARGET, "🌐 JSON-RPC error: {}", e);
    if let Some(handler_err) = e.downcast_ref::<HandlerError>() {
        return resolve_handler_error(answer_id, handler_err);
    }

    if let Some(error) = e.downcast_ref::<JsonRpcError>() {
        return JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(error.error_reason(), error.to_string(), serde_json::Value::Null),
        );
    }

    if let Some(error) = e.downcast_ref::<JwtApiError>() {
        JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(
                JsonRpcErrorReason::ApplicationError(401),
                error.to_string(),
                serde_json::Value::Null,
            ),
        )
    } else {
        JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(
                JsonRpcErrorReason::ApplicationError(ApplicationErrorCode::GeneralError as i32),
                e.to_string(),
                json!({}),
            ),
        )
    }
}

#[repr(i32)]
pub enum ApplicationErrorCode {
    NotFound = 404,
    InvalidRequest = 400,
    TransactionRejected = 1000,
    GeneralError = 500,
}
