// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Debug, future::Future, time::Duration};

use tari_dan_wallet_storage_sqlite::SqliteWalletStore;
use tari_wallet_daemon_client::types::AuthLoginRequest;
use url::Url;
use webauthn_rs::{Webauthn, WebauthnBuilder};

use crate::{
    config::{WalletDaemonAuth, WalletDaemonConfig},
    handlers::auth::{anonym::AnonymousAuth, webauthn::WebAuthnAuth},
    services::WebauthnService,
};

pub mod anonym;
pub mod jwt;
pub mod webauthn;

pub trait Authenticator {
    fn authenticate(&self, request: &AuthLoginRequest) -> impl Future<Output = Result<(), anyhow::Error>>;
}

pub fn create_authenticator(
    config: &WalletDaemonConfig,
    wallet_store: SqliteWalletStore,
) -> anyhow::Result<WalletAuthenticator> {
    match config.authentication {
        WalletDaemonAuth::None => Ok(WalletAuthenticator::None(AnonymousAuth::new())),
        WalletDaemonAuth::WebAuthn => {
            let rp_origin = match config.web_ui_address {
                Some(ui_address) => Url::parse(format!("http://localhost:{}", ui_address.port()).as_str())?,
                None => {
                    let ui_address = WalletDaemonConfig::default().web_ui_address.unwrap();
                    Url::parse(format!("http://localhost:{}", ui_address.port()).as_str())?
                },
            };
            let webauthn = WebauthnBuilder::new("localhost", &rp_origin)?
                .rp_name("Tari Ootle")
                .build()?;
            let webauthn_service = WebauthnService::new(wallet_store, Duration::from_secs(60 * 60));
            Ok(WalletAuthenticator::WebAuthn(WebAuthnAuth::new(
                webauthn,
                webauthn_service,
            )))
        },
    }
}

#[derive(Debug, Clone)]
pub enum WalletAuthenticator {
    None(AnonymousAuth),
    WebAuthn(WebAuthnAuth<SqliteWalletStore>),
}

impl WalletAuthenticator {
    pub fn webauthn(&self) -> Option<&Webauthn> {
        match self {
            WalletAuthenticator::None(_) => None,
            WalletAuthenticator::WebAuthn(auth) => Some(auth.webauthn()),
        }
    }

    pub fn webauthn_service(&self) -> Option<&WebauthnService<SqliteWalletStore>> {
        match self {
            WalletAuthenticator::None(_) => None,
            WalletAuthenticator::WebAuthn(auth) => Some(auth.webauthn_service()),
        }
    }
}

impl Authenticator for WalletAuthenticator {
    async fn authenticate(&self, request: &AuthLoginRequest) -> Result<(), anyhow::Error> {
        match self {
            WalletAuthenticator::None(auth) => auth.authenticate(request).await,
            WalletAuthenticator::WebAuthn(auth) => auth.authenticate(request).await,
        }
    }
}
