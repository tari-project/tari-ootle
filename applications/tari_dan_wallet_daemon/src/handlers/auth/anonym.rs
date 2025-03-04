// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_wallet_daemon_client::types::AuthLoginRequest;

use crate::handlers::auth::Authenticator;

#[derive(Debug, Clone, Copy)]
pub struct AnonymousAuth;

impl AnonymousAuth {
    pub fn new() -> Self {
        Self {}
    }
}

impl Authenticator for AnonymousAuth {
    async fn authenticate(&self, _request: &AuthLoginRequest) -> Result<(), anyhow::Error> {
        Ok(())
    }
}
