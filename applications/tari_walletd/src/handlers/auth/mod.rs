// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Debug, future::Future};

use log::*;
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
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

const LOG_TARGET: &str = "tari::ootle::walletd::auth";

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
            let rp_origin =
                config
                    .webauthn
                    .rp_origin
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| match config.web_ui_address {
                        Some(ui_address) => Url::parse(format!("http://localhost:{}", ui_address.port()).as_str())
                            .expect("url cannot be invalid"),
                        None => {
                            let ui_address = WalletDaemonConfig::default()
                                .web_ui_address
                                .expect("BUG: no default web ui address");
                            Url::parse(format!("http://localhost:{}", ui_address.port()).as_str())
                                .expect("url cannot be invalid")
                        },
                    });
            info!(target: LOG_TARGET, "Using WebAuthn authentication with RP origin: {} TTL: {:?}", rp_origin, config.webauthn.session_ttl);
            let webauthn = WebauthnBuilder::new(&config.webauthn.rp_id, &rp_origin)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "{} RP origin {} does not end with RP ID {}",
                        e,
                        rp_origin,
                        config.webauthn.rp_id
                    )
                })?
                .rp_name("Tari Ootle Wallet")
                .build()?;
            let webauthn_service = WebauthnService::new(wallet_store, config.webauthn.session_ttl);
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
