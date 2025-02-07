// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use crate::handlers::auth::Authenticator;
use crate::services::WebauthnService;
use anyhow::anyhow;
use axum::async_trait;
use std::fmt::Debug;
use std::sync::Arc;
use tari_dan_wallet_sdk::storage::WalletStore;
use tari_wallet_daemon_client::types::AuthLoginRequest;
use webauthn_rs::Webauthn;

#[derive(Debug)]
pub struct WebAuthnAuth<TStore: WalletStore + Debug + Send + Sync>{
    webauthn: Webauthn,
    webauthn_service: Arc<WebauthnService<TStore>>
}

impl<TStore: WalletStore + Debug + Send + Sync> WebAuthnAuth<TStore> {
    pub fn new(
        webauthn: Webauthn,
        webauthn_service: Arc<WebauthnService<TStore>>,
    ) -> Self {
        WebAuthnAuth{
            webauthn,
            webauthn_service
        }
    }
}

#[async_trait]
impl<TStore: WalletStore + Debug + Send + Sync> Authenticator for WebAuthnAuth<TStore> {

    async fn authenticate(&self, request: &AuthLoginRequest) -> Result<(), anyhow::Error> {
        match &request.webauthn_finish_auth_request {
            Some(req) => {
                let credential = req.credential()?;
                let auth_passkey = self.webauthn_service.auth_passkey(req.session_id.as_str()).await?;
                self.webauthn.finish_passkey_authentication(&credential, &auth_passkey)
                    .map_err(|error| {
                        anyhow!("Authentication failed: {error:?}")
                    })?;
                Ok(())
            }
            None => {
                Err(anyhow!("No webauthn finish login request present in request!"))
            }
        }
    }
}

