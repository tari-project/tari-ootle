//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use tari_ootle_wallet_sdk::models::AuthLoginRequestEvent;
use tari_wallet_daemon_client::{
    permissions::JrpcPermission,
    types::{
        AuthGetAllJwtRequest,
        AuthGetAllJwtResponse,
        AuthGetMethodRequest,
        AuthGetMethodResponse,
        AuthLoginAcceptRequest,
        AuthLoginAcceptResponse,
        AuthLoginDenyRequest,
        AuthLoginDenyResponse,
        AuthLoginRequest,
        AuthLoginResponse,
        AuthMethod,
        AuthRevokeTokenRequest,
        AuthRevokeTokenResponse,
    },
};

use crate::{
    config::WalletDaemonAuth,
    handlers::{HandlerContext, auth::Authenticator},
};

// NOTE: most of these do not require a bearer token, either because they are not sensitive or because they validate a
// token in the request body.

pub async fn handle_discover(
    _context: &HandlerContext,
    _token: Option<&Bearer>,
    _value: serde_json::Value,
) -> Result<serde_json::Value, anyhow::Error> {
    Ok(serde_json::from_str(include_str!("../../openrpc.json"))?)
}

pub async fn handle_login_request(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    auth_request: AuthLoginRequest,
) -> Result<AuthLoginResponse, anyhow::Error> {
    context.authenticator().authenticate(&auth_request).await?;
    let jwt = context.jwt_api();
    let (auth_token, valid_for) =
        jwt.generate_auth_token(auth_request.permissions.as_slice().try_into()?, auth_request.duration)?;
    context.notifier().notify(AuthLoginRequestEvent);
    Ok(AuthLoginResponse {
        auth_token,
        valid_for_secs: valid_for.as_secs(),
    })
}

pub async fn handle_login_accept(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    auth_accept_request: AuthLoginAcceptRequest,
) -> Result<AuthLoginAcceptResponse, anyhow::Error> {
    let jwt = context.jwt_api();
    let permissions_token = jwt.grant(auth_accept_request.name, &auth_accept_request.auth_token)?;
    Ok(AuthLoginAcceptResponse { permissions_token })
}

pub async fn handle_login_deny(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    auth_deny_request: AuthLoginDenyRequest,
) -> Result<AuthLoginDenyResponse, anyhow::Error> {
    let jwt = context.jwt_api();
    jwt.deny(&auth_deny_request.auth_token)?;
    Ok(AuthLoginDenyResponse {})
}

pub async fn handle_revoke(
    context: &HandlerContext,
    token: Option<&Bearer>,
    revoke_request: AuthRevokeTokenRequest,
) -> Result<AuthRevokeTokenResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    context.jwt_api().revoke(revoke_request.permission_token_id)?;
    Ok(AuthRevokeTokenResponse {})
}

pub async fn handle_get_all_jwt(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _request: AuthGetAllJwtRequest,
) -> Result<AuthGetAllJwtResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let jwt = context.jwt_api();
    let tokens = jwt.get_tokens()?;
    Ok(AuthGetAllJwtResponse { jwt: tokens })
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
