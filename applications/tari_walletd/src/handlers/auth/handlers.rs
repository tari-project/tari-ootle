//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::{extract::CookieJar, headers::authorization::Bearer};
use axum_jrpc::JsonRpcResponse;
use log::warn;
use tari_ootle_wallet_sdk::models::AuthLoginRequestEvent;
use tari_ootle_walletd_client::{
    permissions::{JrpcPermission, JrpcPermissions},
    types::{
        AuthCredentials, AuthGetMethodRequest, AuthGetMethodResponse, AuthListSessionsRequest,
        AuthListSessionsResponse, AuthLoginRequest, AuthLoginResponse, AuthMethod, AuthRefreshRequest,
        AuthRevokeTokenRequest, AuthRevokeTokenResponse, AuthSessionInfo,
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
    let (permissions, issue_refresh_token, api_key_id) = match request.credentials {
        AuthCredentials::ApiKey(raw_key) => match context.api_key_manager().authenticate(&raw_key).await {
            Ok(key_info) => (JrpcPermissions::from(key_info.permissions), false, Some(key_info.id)),
            Err(crate::services::ApiKeyError::InvalidKey) => {
                warn!("API key authentication failed: key not found");
                return Err(unauthorized("Authentication failed"));
            },
            Err(crate::services::ApiKeyError::Revoked) => {
                warn!("API key authentication failed: key revoked");
                return Err(unauthorized("Authentication failed"));
            },
            Err(err) => return Err(err.into()),
        },
        credentials => {
            context.authenticator().authenticate(&credentials).await?;
            (request.permissions.into(), true, None)
        },
    };
    let jwt = context.jwt_api();
    let claims = match api_key_id {
        Some(api_key_id) => jwt.generate_auth_claims_for_api_key(permissions, api_key_id)?,
        None => jwt.generate_auth_claims(permissions)?,
    };
    let token = jwt.grant(&claims)?;
    let cookie = if issue_refresh_token {
        let refresh_token = context
            .refresh_token_store()
            .new_token(claims.permissions, claims.exp)
            .await;
        let refresh_cookie = refresh_token.into_cookie(REFRESH_TOKEN_COOKIE);
        CookieJar::new().add(refresh_cookie)
    } else {
        CookieJar::new()
    };
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
