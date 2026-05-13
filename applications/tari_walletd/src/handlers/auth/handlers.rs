//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::{extract::CookieJar, headers::authorization::Bearer};
use axum_jrpc::JsonRpcResponse;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tari_ootle_wallet_sdk::{
    models::AuthLoginRequestEvent,
    storage::{ReadableWalletStore, WriteableWalletStore},
};
use tari_ootle_wallet_storage_sqlite::ApiKeyStore;
use tari_ootle_walletd_client::{
    permissions::JrpcPermission,
    types::{
        ApiKeyInfo,
        AuthCreateApiKeyRequest,
        AuthCreateApiKeyResponse,
        AuthCredentials,
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
use zeroize::Zeroizing;

use crate::{
    config::WalletDaemonAuth,
    handlers::{HandlerContext, auth::Authenticator, helpers::unauthorized},
};

pub const REFRESH_TOKEN_COOKIE: &str = "r-tkn";

const API_KEY_PREFIX: &str = "twda_";

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub async fn handle_login_request(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    request: (axum_jrpc::Id, AuthLoginRequest),
) -> Result<(CookieJar, JsonRpcResponse), anyhow::Error> {
    let (answer_id, request) = request;
    // Destructure so each arm can move its field without a non-zeroized copy.
    let AuthLoginRequest { credentials, permissions } = request;

    match credentials {
        // API-key path: move the raw key directly into Zeroizing — no intermediate copy.
        AuthCredentials::ApiKey(raw_key) => {
            let raw_key = Zeroizing::new(raw_key);
            let hash: Vec<u8> = Sha256::digest(raw_key.as_bytes()).to_vec();

            let kid = context.wallet_sdk().store().with_write_tx(|tx| {
                let row = tx
                    .api_keys_get_by_hash(&hash)
                    .map_err(|_| anyhow::anyhow!("Invalid API key"))?;

                // Constant-time comparison guards against timing side-channels.
                if !bool::from(row.key_hash.as_slice().ct_eq(&hash)) {
                    return Err(anyhow::anyhow!("Invalid API key"));
                }

                if row.revoked_at.is_some() {
                    return Err(anyhow::anyhow!("API key has been revoked"));
                }

                tx.api_keys_touch(row.id, now_unix())
                    .map_err(|e| anyhow::anyhow!("Failed to record API key usage: {}", e))?;

                Ok::<_, anyhow::Error>(row.id)
            })?;

            let jwt = context.jwt_api();
            let mut claims = jwt.generate_auth_claims(permissions.into())?;
            claims.kid = Some(kid);
            let token = jwt.grant(&claims)?;
            let refresh_token = context
                .refresh_token_store()
                .new_token(claims.permissions, claims.exp)
                .await;
            let refresh_cookie = refresh_token.into_cookie(REFRESH_TOKEN_COOKIE);
            let cookie = CookieJar::new().add(refresh_cookie);
            context.notifier().notify(AuthLoginRequestEvent);
            Ok((cookie, JsonRpcResponse::success(answer_id, AuthLoginResponse { token })))
        },
        other_credentials => {
            context.authenticator().authenticate(&other_credentials).await?;
            let jwt = context.jwt_api();
            let claims = jwt.generate_auth_claims(permissions.into())?;
            let token = jwt.grant(&claims)?;
            let refresh_token = context
                .refresh_token_store()
                .new_token(claims.permissions, claims.exp)
                .await;

            let refresh_cookie = refresh_token.into_cookie(REFRESH_TOKEN_COOKIE);
            let cookie = CookieJar::new().add(refresh_cookie);
            context.notifier().notify(AuthLoginRequestEvent);
            Ok((cookie, JsonRpcResponse::success(answer_id, AuthLoginResponse { token })))
        },
    }
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

pub async fn handle_create_api_key(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: AuthCreateApiKeyRequest,
) -> Result<AuthCreateApiKeyResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;

    // 32 cryptographically-random bytes → `twda_{base64url-no-pad}`.
    let mut raw_bytes = Zeroizing::new([0u8; 32]);
    rand::rng().fill_bytes(raw_bytes.as_mut());
    let encoded = Zeroizing::new(format!("{}{}", API_KEY_PREFIX, URL_SAFE_NO_PAD.encode(raw_bytes.as_ref())));

    // Only the SHA-256 hash is stored; the raw key is never persisted.
    let key_hash: Vec<u8> = Sha256::digest(encoded.as_bytes()).to_vec();
    let permissions_str = request.permissions.join(",");

    let row = context.wallet_sdk().store().with_write_tx(|tx| {
        tx.api_keys_insert(&request.name, &key_hash, &permissions_str)
            .map_err(anyhow::Error::from)
    })?;

    let key = encoded.as_str().to_owned();
    drop(encoded); // zeroize the raw key before it leaves scope via response
    Ok(AuthCreateApiKeyResponse {
        id: row.id,
        name: row.name,
        key,
        permissions: request.permissions,
        created_at: row.created_at,
    })
}

pub async fn handle_list_api_keys(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _request: AuthListApiKeysRequest,
) -> Result<AuthListApiKeysResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let rows = context.wallet_sdk().store().with_read_tx(|tx| {
        Ok::<_, anyhow::Error>(tx.api_keys_list_all()?)
    })?;

    let keys = rows
        .into_iter()
        .map(|row| ApiKeyInfo {
            id: row.id,
            name: row.name,
            permissions: row
                .permissions
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            created_at: row.created_at,
            last_used_at: row.last_used_at,
            revoked_at: row.revoked_at,
        })
        .collect();

    Ok(AuthListApiKeysResponse { keys })
}

pub async fn handle_revoke_api_key(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: AuthRevokeApiKeyRequest,
) -> Result<AuthRevokeApiKeyResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;

    context.wallet_sdk().store().with_write_tx(|tx| {
        tx.api_keys_revoke(request.id, now_unix()).map_err(anyhow::Error::from)
    })?;

    Ok(AuthRevokeApiKeyResponse {})
}
