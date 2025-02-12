// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Debug, sync::Arc};

use axum::async_trait;
use tari_dan_wallet_sdk::storage::WalletStore;
use tari_wallet_daemon_client::types::AuthLoginRequest;
use webauthn_rs::Webauthn;

use crate::{
    config::WalletDaemonAuth,
    handlers::auth::{anonym::AnonymAuth, webauthn::WebAuthnAuth},
    services::WebauthnService,
};

pub mod anonym;
pub mod webauthn;

#[async_trait]
pub trait Authenticator: Debug + Sync + Send {
    async fn authenticate(&self, request: &AuthLoginRequest) -> Result<(), anyhow::Error>;
}

pub fn get_authenticator<TStore: WalletStore + Debug + Send + Sync + 'static>(
    auth: WalletDaemonAuth,
    webauthn: Webauthn,
    webauthn_service: Arc<WebauthnService<TStore>>,
) -> Arc<dyn Authenticator> {
    match auth {
        WalletDaemonAuth::None => Arc::new(AnonymAuth::new()),
        WalletDaemonAuth::WebAuthn => Arc::new(WebAuthnAuth::new(webauthn, webauthn_service)),
    }
}
