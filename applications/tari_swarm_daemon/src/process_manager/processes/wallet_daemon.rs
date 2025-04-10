//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tari_wallet_daemon_client::{
    types::{
        AccountGetResponse,
        AuthLoginAcceptRequest,
        AuthLoginRequest,
        AuthLoginResponse,
        WebauthnFinishAuthRequest,
    },
    WalletDaemonClient,
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
        let mut client = WalletDaemonClient::connect(format!("http://localhost:{port}"), None)?;
        let AuthLoginResponse { auth_token, .. } = client
            .auth_request(AuthLoginRequest {
                permissions: vec!["Admin".to_string()],
                duration: None,
                webauthn_finish_auth_request,
            })
            .await?;
        let auth_response = client
            .auth_accept(AuthLoginAcceptRequest {
                auth_token,
                name: "Testing Token".to_string(),
            })
            .await?;
        client.set_auth_token(auth_response.permissions_token);

        Ok(client)
    }

    pub async fn get_account_public_key(
        &self,
        name: String,
        webauthn_finish_auth_request: Option<WebauthnFinishAuthRequest>,
    ) -> anyhow::Result<RistrettoPublicKeyBytes> {
        let mut client = self.connect_client(webauthn_finish_auth_request).await?;
        let AccountGetResponse { public_key, .. } = client.accounts_get(name.into()).await?;
        Ok(public_key)
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn instance_mut(&mut self) -> &mut Instance {
        &mut self.instance
    }
}
