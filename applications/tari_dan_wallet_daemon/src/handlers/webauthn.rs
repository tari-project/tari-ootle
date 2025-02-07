// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use crate::config::WalletDaemonAuth;
use crate::handlers::HandlerContext;
use anyhow::anyhow;
use tari_wallet_daemon_client::types::{WebauthnFinishRegisterRequest, WebauthnFinishRegisterResponse, WebauthnStartAuthRequest, WebauthnStartAuthResponse, WebauthnStartRegisterRequest, WebauthnStartRegisterResponse};
use uuid::Uuid;
use webauthn_rs::Webauthn;

const MAX_REGISTERED_USERS: u64 = 1;

pub async fn check_registration_count(context: &HandlerContext) -> Result<(), anyhow::Error> {
    if matches!(context.config().authentication, WalletDaemonAuth::WebAuthn) {
        if let Ok(count) = context.webauthn_service().registration_count().await {
            if count >= MAX_REGISTERED_USERS {
                return Err(anyhow!("Already registered with webauthn!"));
            }
        }
    }

    Ok(())
}

pub async fn webauthn(context: &HandlerContext) -> Result<&Webauthn, anyhow::Error> {
    if matches!(context.config().authentication, WalletDaemonAuth::WebAuthn) {
        return Ok(context.webauthn());
    }

    Err(anyhow!("Webauthn is disabled for this wallet"))
}

pub async fn handle_start_registration(
    context: &HandlerContext,
    _token: Option<String>,
    request: WebauthnStartRegisterRequest,
) -> Result<WebauthnStartRegisterResponse, anyhow::Error> {
    let webauthn = webauthn(context).await?;
    check_registration_count(context).await?;
    let (response, passkey_reg) = webauthn.start_passkey_registration(
        Uuid::new_v4(),
        request.username.as_str(),
        request.username.as_str(),
        None,
    )?;

    let session_id = context.webauthn_service().start_registration(request.username, passkey_reg).await?;

    Ok(
        WebauthnStartRegisterResponse::new(session_id, response.public_key)?
    )
}

pub async fn handle_finish_registration(
    context: &HandlerContext,
    _token: Option<String>,
    request: WebauthnFinishRegisterRequest,
) -> Result<WebauthnFinishRegisterResponse, anyhow::Error> {
    let webauthn = webauthn(context).await?;
    check_registration_count(context).await?;
    let webauthn_service = context.webauthn_service();
    let passkey_reg = webauthn_service.registration_passkey(request.session_id.clone()).await?;
    let passkey = webauthn.finish_passkey_registration(
        &request.credential()?,
        &passkey_reg
    )?;
    webauthn_service.finish_registration(request.session_id, passkey).await?;
    Ok(WebauthnFinishRegisterResponse::new(true))
}

pub async fn handle_start_auth(
    context: &HandlerContext,
    _token: Option<String>,
    request: WebauthnStartAuthRequest,
) -> Result<WebauthnStartAuthResponse, anyhow::Error> {
    let webauthn = webauthn(context).await?;
    let webauthn_service = context.webauthn_service();
    let passkeys = webauthn_service.passkeys(request.username)?;
    let (challenge, passkey_auth) = webauthn.start_passkey_authentication(passkeys.as_slice())?;
    let session_id = webauthn_service.start_authentication(passkey_auth).await?;
    Ok(
        WebauthnStartAuthResponse::new(session_id, challenge)?
    )
}

