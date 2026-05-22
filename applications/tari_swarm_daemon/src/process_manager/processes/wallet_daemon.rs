//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use tari_ootle_wallet_sdk::models::Account;
use tari_ootle_walletd_client::{
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
                permissions: vec!["admin".parse()?],
                credentials: webauthn_finish_auth_request
                    .map(|r| AuthCredentials::WebAuthN(Box::new(r)))
                    .unwrap_or(AuthCredentials::None),
            })
            .await?;
        client.set_auth_token(token);

        Ok(client)
    }

    pub async fn get_account_by_name(&self, name: String) -> anyhow::Result<Account> {
        let mut client = self.connect_client(None).await?;
        let resp = client.accounts_get(name.into()).await?;
        Ok(resp.account)
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn instance_mut(&mut self) -> &mut Instance {
        &mut self.instance
    }
}
