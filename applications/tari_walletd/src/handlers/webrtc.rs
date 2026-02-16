//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{net::SocketAddr, sync::Arc};

use axum_extra::headers::authorization::Bearer;
use axum_jrpc::{
    JrpcResult,
    JsonRpcExtractor,
    JsonRpcResponse,
    error::{JsonRpcError, JsonRpcErrorReason},
};
use log::*;
use tari_wallet_daemon_client::{
    permissions::{JrpcPermission, JrpcPermissions},
    types::{WebRtcStartRequest, WebRtcStartResponse},
};

use super::HandlerContext;
use crate::webrtc::webrtc_start_session;

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::json_rpc";

pub fn handle_start(
    context: Arc<HandlerContext>,
    value: JsonRpcExtractor,
    token: Option<&Bearer>,
    addresses: (SocketAddr, SocketAddr),
) -> JrpcResult {
    let answer_id = value.get_answer_id();
    context.check_auth(token, &[JrpcPermission::StartWebrtc]).map_err(|e| {
        JsonRpcResponse::error(
            answer_id.clone(),
            JsonRpcError::new(
                JsonRpcErrorReason::ApplicationError(401),
                format!("Not authorized: {e}"),
                serde_json::Value::Null,
            ),
        )
    })?;
    let webrtc_start_request = value.parse_params::<WebRtcStartRequest>()?;
    let permissions = serde_json::from_value::<JrpcPermissions>(webrtc_start_request.permissions).map_err(|e| {
        JsonRpcResponse::error(
            answer_id.clone(),
            JsonRpcError::new(
                JsonRpcErrorReason::InvalidRequest,
                e.to_string(),
                serde_json::Value::Null,
            ),
        )
    })?;
    let jwt = context.jwt_api();
    let claim = jwt
        .generate_auth_claims(webrtc_start_request.name, permissions)
        .map_err(|e| {
            JsonRpcResponse::error(
                answer_id.clone(),
                JsonRpcError::new(
                    JsonRpcErrorReason::InternalError,
                    e.to_string(),
                    serde_json::Value::Null,
                ),
            )
        })?;
    let permissions_token = jwt.grant(&claim).map_err(|e| {
        JsonRpcResponse::error(
            answer_id.clone(),
            JsonRpcError::new(
                JsonRpcErrorReason::InternalError,
                e.to_string(),
                serde_json::Value::Null,
            ),
        )
    })?;
    tokio::spawn(async move {
        let (preferred_address, signaling_server_address) = addresses;
        if let Err(err) = webrtc_start_session(
            webrtc_start_request.signaling_server_token,
            permissions_token,
            preferred_address,
            signaling_server_address,
            context.shutdown_signal().clone(),
        )
        .await
        {
            error!(target: LOG_TARGET, "Error starting webrtc session: {}", err);
        }
    });
    Ok(JsonRpcResponse::success(answer_id, WebRtcStartResponse {}))
}
