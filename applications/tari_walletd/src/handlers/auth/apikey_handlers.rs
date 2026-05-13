//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! JSON-RPC handlers for API key management.

use axum_extra::headers::authorization::Bearer;
use tari_ootle_walletd_client::{
    permissions::JrpcPermission,
    types::{
        CreateApiKeyRequest,
        CreateApiKeyResponse,
        ListApiKeysRequest,
        ListApiKeysResponse,
        RevokeApiKeyRequest,
        RevokeApiKeyResponse,
    },
};

use crate::handlers::{HandlerContext, helpers::unauthorized};

/// Create a new API key. Requires Admin permission.
pub async fn handle_create_api_key(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: (axum_jrpc::Id, CreateApiKeyRequest),
) -> Result<CreateApiKeyResponse, anyhow::Error> {
    // Only Admin can create API keys
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let (_answer_id, req) = request;

    // Validate explicit admin grant
    for scope in &req.scopes {
        if scope == &JrpcPermission::Admin && !req.grant_admin {
            return Err(anyhow::anyhow!(
                "Cannot grant Admin scope without explicit 'grant_admin' flag"
            ));
        }
    }

    let generated = context.api_key_store().create_key(req.name, req.scopes).await;

    Ok(CreateApiKeyResponse {
        id: generated.info.id,
        key: generated.raw_key.into(),
        info: generated.info,
    })
}

/// List all active API keys. Requires Admin permission.
pub async fn handle_list_api_keys(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _request: (axum_jrpc::Id, ListApiKeysRequest),
) -> Result<ListApiKeysResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let keys = context.api_key_store().list_keys().await;

    Ok(ListApiKeysResponse { keys })
}

/// Revoke an API key. Requires Admin permission.
pub async fn handle_revoke_api_key(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: (axum_jrpc::Id, RevokeApiKeyRequest),
) -> Result<RevokeApiKeyResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let (_answer_id, req) = request;
    context.api_key_store().revoke_key(req.id).await;

    Ok(RevokeApiKeyResponse {})
}
