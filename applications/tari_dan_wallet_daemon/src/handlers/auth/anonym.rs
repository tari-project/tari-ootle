// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use axum::async_trait;
use tari_wallet_daemon_client::types::AuthLoginRequest;

use crate::handlers::auth::Authenticator;

#[derive(Debug)]
pub struct AnonymAuth;

impl AnonymAuth {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl Authenticator for AnonymAuth {
    async fn authenticate(&self, _request: &AuthLoginRequest) -> Result<(), anyhow::Error> {
        Ok(())
    }
}
