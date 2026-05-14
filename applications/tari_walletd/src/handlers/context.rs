//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use axum_extra::headers::authorization::Bearer;
use tari_ootle_transaction::{Transaction, TransactionBuilder};
use tari_ootle_wallet_sdk::models::WalletEvent;
use tari_ootle_wallet_sdk_services::{
    account_monitor::AccountMonitorHandle,
    notify::Notify,
    transaction_service::TransactionServiceHandle,
};
use tari_ootle_wallet_storage_sqlite::{ApiKeyStore, SqliteWalletStore};
use tari_ootle_walletd_client::permissions::JrpcPermission;
use tari_shutdown::ShutdownSignal;
use tari_utilities::SafePassword;
use webauthn_rs::Webauthn;

use crate::{
    WalletSdk,
    config::WalletDaemonConfig,
    handlers::auth::{
        WalletAuthenticator,
        jwt::{JwtApi, JwtApiError},
    },
    services::{RefreshTokenStore, WebauthnService},
};

#[derive(Debug, Clone)]
pub struct HandlerContext {
    wallet_sdk: WalletSdk,
    notifier: Notify<WalletEvent>,
    transaction_service: TransactionServiceHandle,
    account_monitor: AccountMonitorHandle,
    config: WalletDaemonConfig,
    jwt_secret: SafePassword,
    authenticator: WalletAuthenticator,
    refresh_token_store: RefreshTokenStore,
    shutdown_signal: ShutdownSignal,
}

impl HandlerContext {
    pub fn new(
        wallet_sdk: WalletSdk,
        notifier: Notify<WalletEvent>,
        transaction_service: TransactionServiceHandle,
        account_monitor: AccountMonitorHandle,
        config: WalletDaemonConfig,
        authenticator: WalletAuthenticator,
        jwt_secret: SafePassword,
        shutdown_signal: ShutdownSignal,
    ) -> Self {
        Self {
            wallet_sdk,
            notifier,
            transaction_service,
            account_monitor,
            config,
            authenticator,
            refresh_token_store: RefreshTokenStore::new(Duration::from_secs(60 * 60)),
            jwt_secret,
            shutdown_signal,
        }
    }

    pub fn notifier(&self) -> &Notify<WalletEvent> {
        &self.notifier
    }

    pub fn wallet_sdk(&self) -> &WalletSdk {
        &self.wallet_sdk
    }

    pub fn check_auth(&self, token: Option<&Bearer>, permissions: &[JrpcPermission]) -> Result<(), JwtApiError> {
        let claims = self.jwt_api().check_auth_returning_claims(token, permissions)?;
        // For API-key-backed JWTs, verify the key has not been revoked since this token was issued.
        if let Some(kid) = claims.kid {
            let active = self
                .wallet_sdk()
                .store()
                .with_read_tx(|tx| {
                    Ok::<bool, anyhow::Error>(
                        tx.api_keys_list_all()?.into_iter().any(|k| k.id == kid && k.revoked_at.is_none()),
                    )
                })
                .unwrap_or(false);
            if !active {
                return Err(JwtApiError::AccessDeniedInvalidBearerToken);
            }
        }
        Ok(())
    }

    pub fn jwt_api(&self) -> JwtApi<'_> {
        JwtApi::new(self.config().jwt_expiry, &self.jwt_secret)
    }

    pub fn shutdown_signal(&self) -> &ShutdownSignal {
        &self.shutdown_signal
    }

    pub fn account_monitor(&self) -> &AccountMonitorHandle {
        &self.account_monitor
    }

    pub fn transaction_service(&self) -> &TransactionServiceHandle {
        &self.transaction_service
    }

    pub fn config(&self) -> &WalletDaemonConfig {
        &self.config
    }

    pub fn authenticator(&self) -> &WalletAuthenticator {
        &self.authenticator
    }

    pub fn refresh_token_store(&self) -> &RefreshTokenStore {
        &self.refresh_token_store
    }

    pub fn webauthn_service(&self) -> Option<&WebauthnService<SqliteWalletStore>> {
        self.authenticator.webauthn_service()
    }

    pub fn webauthn(&self) -> Option<&Webauthn> {
        self.authenticator.webauthn()
    }

    /// Returns a TransactionBuilder with the current network configured.
    pub fn transaction_builder(&self) -> TransactionBuilder {
        Transaction::builder(self.config().network.as_byte())
    }
}
