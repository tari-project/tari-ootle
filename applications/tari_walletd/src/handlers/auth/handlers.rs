//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::{extract::CookieJar, headers::authorization::Bearer};
use axum_jrpc::JsonRpcResponse;
use tari_ootle_wallet_sdk::models::AuthLoginRequestEvent;
use tari_ootle_walletd_client::{
    permissions::JrpcPermission,
    types::{
        AuthCredentials,
        AuthGetMethodRequest,
        AuthGetMethodResponse,
        AuthListSessionsRequest,
        AuthListSessionsResponse,
        AuthLoginRequest,
        AuthLoginResponse,
        AuthMethod,
        AuthRefreshRequest,
        AuthRevokeTokenRequest,
        AuthRevokeTokenResponse,
        AuthSessionInfo,
    },
};

use crate::{
    config::WalletDaemonAuth,
    handlers::{
        HandlerContext,
        auth::{Authenticator, api_keys},
        helpers::unauthorized,
    },
};

pub const REFRESH_TOKEN_COOKIE: &str = "r-tkn";

pub async fn handle_login_request(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    request: (axum_jrpc::Id, AuthLoginRequest),
) -> Result<(CookieJar, JsonRpcResponse), anyhow::Error> {
    let (answer_id, request) = request;

    // API key credentials are handled out-of-band from the `Authenticator`
    // trait: that trait returns `Result<(), _>` with no notion of "granted
    // scopes" because for webauthn the scopes come from the request, not
    // the credentials. With API keys it's the reverse — the scopes come
    // from the stored key row and the client's `request.permissions` field
    // is IGNORED. Doing this check before the `Authenticator` call lets us
    // use a single `auth.request` JSON-RPC entry point for all credential
    // types.
    let granted_permissions = match &request.credentials {
        AuthCredentials::ApiKey(raw_key) => {
            api_keys::authenticate_api_key(context.wallet_sdk().store(), raw_key).await?
        },
        _ => {
            context.authenticator().authenticate(&request.credentials).await?;
            request.permissions.into()
        },
    };

    let jwt = context.jwt_api();
    let claims = jwt.generate_auth_claims(granted_permissions)?;
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
