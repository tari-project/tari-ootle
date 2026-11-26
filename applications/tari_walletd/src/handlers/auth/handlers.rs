//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::{extract::CookieJar, headers::authorization::Bearer};
use axum_jrpc::JsonRpcResponse;
use tari_ootle_wallet_sdk::models::AuthLoginRequestEvent;
use tari_ootle_walletd_client::{
    permissions::Permission,
    types::{
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
        auth::Authenticator,
        helpers::{not_found, unauthorized},
    },
};

pub const REFRESH_TOKEN_COOKIE: &str = "r-tkn";

pub async fn handle_login_request(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    request: (axum_jrpc::Id, AuthLoginRequest),
) -> Result<(CookieJar, JsonRpcResponse), anyhow::Error> {
    let (answer_id, request) = request;

    context.authenticator().authenticate(&request.credentials).await?;
    let jwt = context.jwt_api();
    let claims = jwt.generate_auth_claims(request.permissions.into())?;
    let token = jwt.grant(&claims)?;
    let refresh_token = context.refresh_token_store().new_token(claims.permissions).await;
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
    context.authorize_user_only(token, &[Permission::Admin])?;
    if !context
        .refresh_token_store()
        .revoke_token(&revoke_request.refresh_token_id)
        .await
    {
        return Err(not_found(format!(
            "Refresh token {} not found",
            revoke_request.refresh_token_id
        )));
    }
    Ok(AuthRevokeTokenResponse {})
}

pub async fn handle_list_sessions(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _request: AuthListSessionsRequest,
) -> Result<AuthListSessionsResponse, anyhow::Error> {
    context.authorize_user_only(token, &[Permission::Admin])?;
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

#[cfg(test)]
mod tests {
    use axum_extra::headers::Authorization;
    use axum_jrpc::{
        Id,
        error::{JsonRpcError, JsonRpcErrorReason},
    };
    use tari_ootle_walletd_client::{
        permissions::{Crud, Permissions},
        types::{AuthCredentials, RefreshTokenHash},
    };

    use super::*;
    use crate::{
        handlers::{auth::jwt::AuthError, test_support::TestDaemon},
        server::ApplicationErrorCode,
    };

    #[tokio::test]
    async fn list_sessions_and_revoke_require_interactive_admin_session() {
        let daemon = TestDaemon::start().await;
        let non_admin_claims = daemon
            .context
            .jwt_api()
            .generate_auth_claims(Permissions::from(vec![Permission::Settings(Crud::Read)]))
            .unwrap();
        let non_admin = Authorization::<Bearer>::bearer(&daemon.context.jwt_api().grant(&non_admin_claims).unwrap())
            .unwrap()
            .0;
        let api_key_admin = daemon.api_key("admin-agent", "admin");
        let delegated_admin = daemon.delegated_session("admin");
        let dummy_id = RefreshTokenHash::new([0x11; 32]);

        let err = handle_list_sessions(&daemon.context, None, AuthListSessionsRequest {})
            .await
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<AuthError>(),
            Some(AuthError::AccessDeniedNoBearerToken)
        ));

        let err = handle_list_sessions(&daemon.context, Some(&non_admin), AuthListSessionsRequest {})
            .await
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<AuthError>(),
            Some(AuthError::InsufficientPermissions { .. })
        ));

        for forbidden in [&api_key_admin, &delegated_admin, &daemon.cookie] {
            let err = handle_list_sessions(&daemon.context, Some(forbidden), AuthListSessionsRequest {})
                .await
                .unwrap_err();
            assert!(
                matches!(err.downcast_ref::<AuthError>(), Some(AuthError::UserAuthOnly)),
                "list_sessions must reject non-interactive bearer: {err}"
            );

            let err = handle_revoke(&daemon.context, Some(forbidden), AuthRevokeTokenRequest {
                refresh_token_id: dummy_id,
            })
            .await
            .unwrap_err();
            assert!(
                matches!(err.downcast_ref::<AuthError>(), Some(AuthError::UserAuthOnly)),
                "revoke must reject non-interactive bearer: {err}"
            );
        }
    }

    #[tokio::test]
    async fn login_records_refresh_session_expiry_and_revoke_rejects_missing_tokens() {
        let daemon = TestDaemon::start().await;
        let jwt_claims = daemon
            .context
            .jwt_api()
            .generate_auth_claims(Permissions::from(vec![Permission::Admin]))
            .unwrap();

        let (_cookies, _response) = handle_login_request(
            &daemon.context,
            None,
            (Id::Num(1), AuthLoginRequest {
                permissions: vec![Permission::Admin],
                credentials: AuthCredentials::None,
            }),
        )
        .await
        .unwrap();

        let listed = handle_list_sessions(&daemon.context, Some(&daemon.session), AuthListSessionsRequest {})
            .await
            .unwrap();
        assert_eq!(listed.sessions.len(), 1);
        let session_info = &listed.sessions[0];
        assert!(
            session_info.exp > jwt_claims.exp + 1800,
            "session exp ({}) must reflect refresh session lifetime, not short-lived JWT exp ({})",
            session_info.exp,
            jwt_claims.exp
        );

        handle_revoke(&daemon.context, Some(&daemon.session), AuthRevokeTokenRequest {
            refresh_token_id: session_info.id,
        })
        .await
        .unwrap();

        let err = handle_revoke(&daemon.context, Some(&daemon.session), AuthRevokeTokenRequest {
            refresh_token_id: session_info.id,
        })
        .await
        .unwrap_err();
        let jrpc = err.downcast_ref::<JsonRpcError>().expect("JsonRpcError");
        assert!(matches!(
            jrpc.error_reason(),
            JsonRpcErrorReason::ApplicationError(code) if code == ApplicationErrorCode::NotFound as i32
        ));
    }
}
