// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use crate::handlers::auth::Authenticator;
use axum::async_trait;
use std::fmt::Debug;
use tari_wallet_daemon_client::types::AuthLoginRequest;

#[derive(Debug)]
pub struct WebAuthnAuth;

impl WebAuthnAuth {
    pub fn new() -> Self {
        WebAuthnAuth{}
    }
}

#[async_trait]
impl Authenticator for WebAuthnAuth {

    async fn authenticate(&self, request: &AuthLoginRequest) -> Result<(), anyhow::Error> {
        todo!()
    }
}

