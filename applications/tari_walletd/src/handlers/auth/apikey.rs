//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! API Key based authenticator for agent-friendly wallet access.

use tari_ootle_walletd_client::types::AuthCredentials;

use super::Authenticator;

pub struct ApiKeyAuth;

impl ApiKeyAuth {
    pub fn new() -> Self {
        Self
    }
}

impl Authenticator for ApiKeyAuth {
    async fn authenticate(&self, credentials: &AuthCredentials) -> Result<(), anyhow::Error> {
        match credentials {
            AuthCredentials::ApiKey(key) if !key.is_empty() => Ok(()),
            _ => Err(anyhow::anyhow!("Invalid API key credentials")),
        }
    }
}
