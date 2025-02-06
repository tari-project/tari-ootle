// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use crate::config::WalletDaemonAuth;
use crate::handlers::HandlerContext;
use anyhow::anyhow;
use tari_wallet_daemon_client::types::{WebauthnFinishRegisterRequest, WebauthnFinishRegisterResponse, WebauthnStartRegisterRequest, WebauthnStartRegisterResponse};
use uuid::Uuid;
use webauthn_rs::Webauthn;

pub async fn webauthn(context: &HandlerContext) -> Result<&Webauthn, anyhow::Error> {
    if matches!(context.config().authentication, WalletDaemonAuth::WebAuthn) && context.webauthn().is_some() {
        if let Ok(count) = context.webauthn_service().registration_count().await {
            if count > 0 {
                return Err(anyhow!("Already registered with webauthn!"));
            }
        }
        return Ok(context.webauthn().unwrap());
    }

    Err(anyhow!("Webauthn is disabled for this wallet"))
}

pub async fn handle_start_registration(
    context: &HandlerContext,
    _token: Option<String>,
    request: WebauthnStartRegisterRequest,
) -> Result<WebauthnStartRegisterResponse, anyhow::Error> {
    let webauthn = webauthn(context).await?;
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
    let webauthn_service = context.webauthn_service();
    let passkey_reg = webauthn_service.registration_passkey(request.session_id).await?;
    let passkey = webauthn.finish_passkey_registration(
        &request.credential()?,
        &passkey_reg
    )?;
    webauthn_service.finish_registration()
    Ok(WebauthnFinishRegisterResponse::new(true))
}

