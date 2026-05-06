//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::{SystemTime, UNIX_EPOCH};

use axum_extra::{extract::CookieJar, headers::authorization::Bearer};
use axum_jrpc::JsonRpcResponse;
use rand::RngCore;
use sha2::{Digest, Sha256};
use tari_ootle_wallet_sdk::{
    models::{AuthLoginRequestEvent, NewApiKeyModel},
    storage::{WalletStore, WalletStoreReader, WalletStoreWriter},
};
use tari_ootle_walletd_client::{
    permissions::{JrpcPermission, JrpcPermissions},
    types::{
        AuthApiKeyInfo,
        AuthCreateApiKeyRequest,
        AuthCreateApiKeyResponse,
        AuthGetMethodRequest,
        AuthGetMethodResponse,
        AuthListApiKeysRequest,
        AuthListApiKeysResponse,
        AuthListSessionsRequest,
        AuthListSessionsResponse,
        AuthLoginRequest,
        AuthLoginResponse,
        AuthMethod,
        AuthRefreshRequest,
        AuthRevokeApiKeyRequest,
        AuthRevokeApiKeyResponse,
        AuthRevokeTokenRequest,
        AuthRevokeTokenResponse,
        AuthSessionInfo,
    },
};

use crate::{
    config::WalletDaemonAuth,
    handlers::{HandlerContext, auth::Authenticator, helpers::unauthorized},
};

pub const REFRESH_TOKEN_COOKIE: &str = "r-tkn";

pub async fn handle_login_request(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    request: (axum_jrpc::Id, AuthLoginRequest),
) -> Result<(CookieJar, JsonRpcResponse), anyhow::Error> {
    let (answer_id, request) = request;
    if request.credentials.as_api_key().is_some() {
        let response = login_with_api_key(context, request).await?;
        context.notifier().notify(AuthLoginRequestEvent);
        return Ok((CookieJar::new(), JsonRpcResponse::success(answer_id, response)));
    }

    context.authenticator().authenticate(&request.credentials).await?;
    let jwt = context.jwt_api();
    let claims = jwt.generate_auth_claims(request.permissions.into())?;
    let token = jwt.grant(&claims)?;
    let refresh_token = context
        .refresh_token_store()
        .new_token(claims.permissions, claims.exp)
        .await;

    let refresh_cookie = refresh_token.into_cookie(REFRESH_TOKEN_COOKIE);
    let cookie = CookieJar::new().add(refresh_cookie);
    context.notifier().notify(AuthLoginRequestEvent);
    Ok((cookie, JsonRpcResponse::success(answer_id, AuthLoginResponse { token })))
}

pub async fn handle_token_refresh(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    cookies: Option<CookieJar>,
    _request: AuthRefreshRequest,
) -> Result<AuthLoginResponse, anyhow::Error> {
    let refresh_token = cookies
        .as_ref()
        .ok_or_else(|| unauthorized("No cookies"))?
        .get(REFRESH_TOKEN_COOKIE)
        .ok_or_else(|| unauthorized("No refresh token"))?;
    let Some(claim) = context
        .refresh_token_store()
        .validate_token_str(refresh_token.value())
        .await
    else {
        return Err(unauthorized("Invalid or expired refresh token"));
    };

    let jwt = context.jwt_api();
    let claims = jwt.generate_auth_claims(claim.permissions).map_err(unauthorized)?;
    let permissions_token = jwt.grant(&claims).map_err(unauthorized)?;
    context.notifier().notify(AuthLoginRequestEvent);
    Ok(AuthLoginResponse {
        token: permissions_token,
    })
}

pub async fn handle_revoke(
    context: &HandlerContext,
    token: Option<&Bearer>,
    revoke_request: AuthRevokeTokenRequest,
) -> Result<AuthRevokeTokenResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    context
        .refresh_token_store()
        .revoke_token(&revoke_request.refresh_token_id)
        .await;
    Ok(AuthRevokeTokenResponse {})
}

pub async fn handle_create_api_key(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: AuthCreateApiKeyRequest,
) -> Result<AuthCreateApiKeyResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    if request.name.trim().is_empty() {
        anyhow::bail!("API key name is required");
    }
    if request.permissions.is_empty() {
        anyhow::bail!("At least one permission is required");
    }
    if request.permissions.contains(&JrpcPermission::Admin) && !request.allow_admin {
        anyhow::bail!("Granting Admin to an API key requires explicit confirmation");
    }

    let id = uuid::Uuid::new_v4().to_string();
    let secret = generate_api_key_secret();
    let api_key = format!("twda_{}_{}", id, secret);
    let key_hash = hash_api_key(&api_key);
    let permissions = request.permissions.iter().map(ToString::to_string).collect::<Vec<_>>();

    context.wallet_sdk().store().with_write_tx(|tx| {
        tx.api_keys_insert(NewApiKeyModel {
            id: id.clone(),
            name: request.name.trim().to_string(),
            key_hash: key_hash.to_vec(),
            permissions,
        })
    })?;

    let created_at = unix_timestamp_secs()?;
    Ok(AuthCreateApiKeyResponse {
        id,
        name: request.name.trim().to_string(),
        api_key,
        permissions: request.permissions,
        created_at,
    })
}

pub async fn handle_list_api_keys(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _request: AuthListApiKeysRequest,
) -> Result<AuthListApiKeysResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let api_keys = context
        .wallet_sdk()
        .store()
        .with_read_tx(|tx| tx.api_keys_list_active())?
        .into_iter()
        .map(|api_key| {
            Ok(AuthApiKeyInfo {
                id: api_key.id,
                name: api_key.name,
                permissions: parse_permissions(api_key.permissions)?,
                created_at: api_key.created_at,
                last_used_at: api_key.last_used_at,
            })
        })
        .collect::<Result<Vec<_>, anyhow::Error>>()?;
    Ok(AuthListApiKeysResponse { api_keys })
}

pub async fn handle_revoke_api_key(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: AuthRevokeApiKeyRequest,
) -> Result<AuthRevokeApiKeyResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    context
        .wallet_sdk()
        .store()
        .with_write_tx(|tx| tx.api_keys_revoke(&request.id))?;
    Ok(AuthRevokeApiKeyResponse {})
}

pub async fn handle_list_sessions(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _request: AuthListSessionsRequest,
) -> Result<AuthListSessionsResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    context.refresh_token_store().clear_expired().await;
    let sessions = context
        .refresh_token_store()
        .to_vec()
        .await
        .into_iter()
        .map(|(id, data)| AuthSessionInfo {
            id,
            permissions: data.permissions.into_vec(),
            exp: data.exp,
        })
        .collect();
    Ok(AuthListSessionsResponse { sessions })
}

pub async fn handle_get_auth_method(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    _request: AuthGetMethodRequest,
) -> Result<AuthGetMethodResponse, anyhow::Error> {
    Ok(AuthGetMethodResponse {
        method: match context.config().authentication {
            WalletDaemonAuth::None => AuthMethod::None,
            WalletDaemonAuth::WebAuthn => AuthMethod::Webauthn,
        },
    })
}

async fn login_with_api_key(
    context: &HandlerContext,
    request: AuthLoginRequest,
) -> Result<AuthLoginResponse, anyhow::Error> {
    let credentials = request
        .credentials
        .as_api_key()
        .ok_or_else(|| unauthorized("API key credentials are required"))?;
    let api_key_id = parse_api_key_id(&credentials.api_key)?;
    let api_key = context
        .wallet_sdk()
        .store()
        .with_read_tx(|tx| tx.api_keys_get_active(api_key_id))?;
    let supplied_hash = hash_api_key(&credentials.api_key);
    if !hashes_equal(&supplied_hash, &api_key.key_hash) {
        return Err(unauthorized("Invalid API key"));
    }

    let stored_permissions = JrpcPermissions::try_from(api_key.permissions.as_slice())?;
    let requested_permissions = JrpcPermissions::from(request.permissions);
    if !stored_permissions.has_permission(&JrpcPermission::Admin) && !requested_permissions.is_subset_of(&stored_permissions) {
        return Err(unauthorized("API key does not grant all requested permissions"));
    }

    context
        .wallet_sdk()
        .store()
        .with_write_tx(|tx| tx.api_keys_touch_last_used(&api_key.id))?;

    let jwt = context.jwt_api();
    let mut claims = jwt.generate_auth_claims(requested_permissions)?;
    claims.api_key_id = Some(api_key.id);
    let token = jwt.grant(&claims)?;
    Ok(AuthLoginResponse { token })
}

fn parse_permissions(permissions: Vec<String>) -> Result<Vec<JrpcPermission>, anyhow::Error> {
    permissions
        .iter()
        .map(|permission| permission.parse().map_err(Into::into))
        .collect()
}

fn parse_api_key_id(api_key: &str) -> Result<&str, anyhow::Error> {
    let mut parts = api_key.splitn(3, '_');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("twda"), Some(id), Some(secret)) if !id.is_empty() && !secret.is_empty() => Ok(id),
        _ => Err(unauthorized("Invalid API key format")),
    }
}

fn generate_api_key_secret() -> String {
    let mut secret = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut secret);
    hex::encode(secret)
}

fn hash_api_key(api_key: &str) -> [u8; 32] {
    let digest = Sha256::digest(api_key.as_bytes());
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&digest);
    bytes
}

fn hashes_equal(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.iter().zip(right).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

fn unix_timestamp_secs() -> Result<u64, anyhow::Error> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
