// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_wallet_daemon_client::types::{
    WebauthnAlreadyRegisteredRequest,
    WebauthnAlreadyRegisteredResponse,
    WebauthnFinishRegisterRequest,
    WebauthnFinishRegisterResponse,
    WebauthnStartAuthRequest,
    WebauthnStartAuthResponse,
    WebauthnStartRegisterRequest,
    WebauthnStartRegisterResponse,
};
use uuid::Uuid;
use webauthn_rs::Webauthn;

use crate::{
    handlers::{HandlerContext, helpers::invalid_request},
    services::WebauthnService,
};

fn is_user_already_registered(context: &HandlerContext, username: &str) -> Result<bool, anyhow::Error> {
    let is_registered = webauthn_service(context)?.is_user_registered(username)?;
    Ok(is_registered)
}

fn assert_user_not_registered(context: &HandlerContext, username: &str) -> Result<(), anyhow::Error> {
    if is_user_already_registered(context, username)? {
        return Err(invalid_request("User is already registered"));
    }

    Ok(())
}

fn webauthn(context: &HandlerContext) -> Result<&Webauthn, anyhow::Error> {
    context
        .webauthn()
        .ok_or_else(|| invalid_request("Webauthn is disabled for this wallet"))
}

fn webauthn_service(context: &HandlerContext) -> Result<&WebauthnService<SqliteWalletStore>, anyhow::Error> {
    context
        .webauthn_service()
        .ok_or_else(|| invalid_request("Webauthn is disabled for this wallet"))
}

pub async fn handle_already_registered(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    request: WebauthnAlreadyRegisteredRequest,
) -> Result<WebauthnAlreadyRegisteredResponse, anyhow::Error> {
    webauthn(context)?;
    let registered = is_user_already_registered(context, &request.username)?;
    Ok(WebauthnAlreadyRegisteredResponse { registered })
}

pub async fn handle_start_registration(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    request: WebauthnStartRegisterRequest,
) -> Result<WebauthnStartRegisterResponse, anyhow::Error> {
    let webauthn = webauthn(context)?;
    assert_user_not_registered(context, &request.username)?;
    let (response, passkey_reg) = webauthn.start_passkey_registration(
        Uuid::new_v4(),
        request.username.as_str(),
        request.username.as_str(),
        None,
    )?;

    let session_id = webauthn_service(context)?
        .start_registration(request.username, passkey_reg)
        .await?;

    Ok(WebauthnStartRegisterResponse {
        session_id,
        public_key: response.public_key,
    })
}

pub async fn handle_finish_registration(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    request: WebauthnFinishRegisterRequest,
) -> Result<WebauthnFinishRegisterResponse, anyhow::Error> {
    let webauthn = webauthn(context)?;
    let webauthn_service = webauthn_service(context)?;
    let session_data = webauthn_service.get_session(&request.session_id).await?;
    assert_user_not_registered(context, session_data.username())?;
    let passkey = webauthn.finish_passkey_registration(&request.credential, session_data.passkey_reg())?;
    webauthn_service
        .finish_registration(request.session_id, passkey)
        .await?;
    Ok(WebauthnFinishRegisterResponse {})
}

pub async fn handle_start_auth(
    context: &HandlerContext,
    _token: Option<&Bearer>,
    request: WebauthnStartAuthRequest,
) -> Result<WebauthnStartAuthResponse, anyhow::Error> {
    let webauthn = webauthn(context)?;
    let webauthn_service = webauthn_service(context)?;
    let passkeys = webauthn_service.passkeys(request.username)?;
    let (challenge, passkey_auth) = webauthn.start_passkey_authentication(passkeys.as_slice())?;
    let session_id = webauthn_service.start_authentication(passkey_auth).await?;
    Ok(WebauthnStartAuthResponse { session_id, challenge })
}
