//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use tari_ootle_wallet_sdk::models::KeyBranch;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tari_wallet_daemon_client::{
    WalletDaemonClient,
    types::{AuthCredentials, AuthLoginRequest, AuthLoginResponse, WebauthnFinishAuthRequest},
};

use crate::process_manager::Instance;

pub struct WalletDaemonProcess {
    instance: Instance,
}

impl WalletDaemonProcess {
    pub fn new(instance: Instance) -> Self {
        Self { instance }
    }

    async fn connect_client(
        &self,
        webauthn_finish_auth_request: Option<WebauthnFinishAuthRequest>,
    ) -> anyhow::Result<WalletDaemonClient> {
        let port = self
            .instance
            .allocated_ports()
            .get("jrpc")
            .ok_or_else(|| anyhow!("No wallet JSON-RPC port allocated"))?;
        let mut client = WalletDaemonClient::connect(format!("http://localhost:{port}/json_rpc"), None)?;
        let AuthLoginResponse { token } = client
            .auth_request(AuthLoginRequest {
                permissions: vec!["Admin".parse()?],
                credentials: webauthn_finish_auth_request
                    .map(|r| AuthCredentials::WebAuthN(Box::new(r)))
                    .unwrap_or(AuthCredentials::None),
            })
            .await?;
        client.set_auth_token(token);

        Ok(client)
    }

    pub async fn create_nonce_key(&self) -> anyhow::Result<(RistrettoPublicKeyBytes, u64)> {
        let mut client = self.connect_client(None).await?;
        let response = client.create_key(KeyBranch::Nonce).await.map_err(|e| anyhow!(e))?;
        Ok((response.public_key, response.id))
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn instance_mut(&mut self) -> &mut Instance {
        &mut self.instance
    }
}
