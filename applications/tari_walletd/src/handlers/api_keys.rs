//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use axum_jrpc::{
    JsonRpcResponse,
    error::{JsonRpcError, JsonRpcErrorReason},
};
use tari_ootle_walletd_client::{
    permissions::JrpcPermission,
    types::{ApiKeyCreateRequest, ApiKeyCreateResponse, ApiKeyListResponse, ApiKeyRevokeRequest},
};

use super::context::HandlerContext;
use crate::services::ApiKeyError;

pub async fn handle_api_key_create(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: (axum_jrpc::Id, ApiKeyCreateRequest),
) -> Result<JsonRpcResponse, anyhow::Error> {
    let (answer_id, request) = request;
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let caller_permissions = context
        .jwt_api()
        .claims_from_bearer(token.ok_or_else(|| anyhow::anyhow!("No bearer token"))?)?
        .permissions;

    match context
        .api_key_manager()
        .create(
            request.name,
            request.permissions,
            &caller_permissions,
            request.grant_admin,
        )
        .await
    {
        Ok((key_info, plaintext_key)) => Ok(JsonRpcResponse::success(
            answer_id,
            ApiKeyCreateResponse {
                id: key_info.id,
                name: key_info.name,
                permissions: key_info.permissions,
                created_at: key_info.created_at,
                expires_at: key_info.expires_at,
                key: plaintext_key,
            },
        )),
        Err(err) => Ok(map_api_key_create_error(answer_id, err)),
    }
}

pub async fn handle_api_key_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: (axum_jrpc::Id, serde_json::Value),
) -> Result<JsonRpcResponse, anyhow::Error> {
    let (answer_id, _request) = request;
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let keys = context
        .api_key_manager()
        .list()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list API keys: {}", e))?;

    let response_keys = keys
        .into_iter()
        .map(|k| tari_ootle_walletd_client::types::ApiKeyInfo {
            id: k.id,
            name: k.name,
            permissions: k.permissions,
            created_at: k.created_at,
            expires_at: k.expires_at,
            last_used: k.last_used,
            revoked: k.revoked,
        })
        .collect();

    Ok(JsonRpcResponse::success(
        answer_id,
        ApiKeyListResponse { keys: response_keys },
    ))
}

pub async fn handle_api_key_revoke(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: (axum_jrpc::Id, ApiKeyRevokeRequest),
) -> Result<JsonRpcResponse, anyhow::Error> {
    let (answer_id, request) = request;
    context.check_auth(token, &[JrpcPermission::Admin])?;
    match context.api_key_manager().revoke(&request.id).await {
        Ok(()) => Ok(JsonRpcResponse::success(
            answer_id,
            serde_json::json!({ "revoked": true }),
        )),
        Err(ApiKeyError::NotFound) => Ok(JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(
                JsonRpcErrorReason::ApplicationError(-32004),
                "API key not found",
                serde_json::Value::Null,
            ),
        )),
        Err(err) => Err(err.into()),
    }
}

fn map_api_key_create_error(answer_id: axum_jrpc::Id, err: ApiKeyError) -> JsonRpcResponse {
    match err {
        ApiKeyError::PermissionDenied => JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(
                JsonRpcErrorReason::ApplicationError(-32001),
                "Permission denied",
                serde_json::Value::Null,
            ),
        ),
        ApiKeyError::ScopeExceedsGrantor => JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(
                JsonRpcErrorReason::ApplicationError(-32002),
                "Requested scopes exceed grantor permissions",
                serde_json::Value::Null,
            ),
        ),
        ApiKeyError::AdminScopeRequiresConfirmation => JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(
                JsonRpcErrorReason::ApplicationError(-32003),
                "Admin scope requires explicit confirmation",
                serde_json::Value::Null,
            ),
        ),
        err => JsonRpcResponse::error(
            answer_id,
            JsonRpcError::new(
                JsonRpcErrorReason::ApplicationError(-32000),
                err.to_string(),
                serde_json::Value::Null,
            ),
        ),
    }
}
