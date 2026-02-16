// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_wallet_daemon_client::types::AuthCredentials;

use crate::handlers::auth::Authenticator;

#[derive(Debug, Clone, Copy)]
pub struct AnonymousAuth;

impl AnonymousAuth {
    pub fn new() -> Self {
        Self {}
    }
}

impl Authenticator for AnonymousAuth {
    async fn authenticate(&self, cred: &AuthCredentials) -> Result<(), anyhow::Error> {
        match cred {
            AuthCredentials::None => Ok(()),
            _ => Err(anyhow::anyhow!("Invalid credentials for anonymous authentication!")),
        }
    }
}
