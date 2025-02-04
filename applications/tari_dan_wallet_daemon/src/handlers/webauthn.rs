// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use crate::handlers::HandlerContext;
use crate::services::AuthLoginRequestEvent;
use tari_wallet_daemon_client::types::{WebauthnStartRegisterRequest, WebauthnStartRegisterResponse};


pub async fn handle_start_registration(
    context: &HandlerContext,
    _token: Option<String>,
    request: WebauthnStartRegisterRequest,
) -> Result<WebauthnStartRegisterResponse, anyhow::Error> {
    
    let jwt = context.wallet_sdk().jwt_api();
    let (auth_token, valid_for) =
        jwt.generate_auth_token(auth_request.permissions.as_slice().try_into()?, auth_request.duration)?;
    context.notifier().notify(AuthLoginRequestEvent);
    Ok(WebauthnStartRegisterResponse {
    })
}

