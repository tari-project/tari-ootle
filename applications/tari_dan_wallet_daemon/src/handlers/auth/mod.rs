// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use crate::config::WalletDaemonAuth;
use crate::handlers::auth::anonym::AnonymAuth;
use crate::handlers::auth::webauthn::WebAuthnAuth;
use axum::async_trait;
use std::fmt::Debug;
use std::sync::Arc;
use tari_wallet_daemon_client::types::AuthLoginRequest;

pub mod anonym;
pub mod webauthn;

#[async_trait]
pub trait Authenticator: Debug + Sync + Send {
    async fn authenticate(&self, request: &AuthLoginRequest) -> Result<(), anyhow::Error>;
}

pub fn get_authenticator(auth: &WalletDaemonAuth) -> Arc<dyn Authenticator> {
    match auth {
        WalletDaemonAuth::None => {
            Arc::new(AnonymAuth::new())
        }
        WalletDaemonAuth::WebAuthn => {
            Arc::new(WebAuthnAuth::new())
        }
    }
}