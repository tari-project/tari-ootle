//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::headers::authorization::Bearer;
use tari_ootle_wallet_sdk::WalletSdk;
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_shutdown::ShutdownSignal;
use tari_transaction::{Transaction, TransactionBuilder};
use tari_wallet_daemon_client::permissions::JrpcPermission;
use webauthn_rs::Webauthn;

use crate::{
    config::WalletDaemonConfig,
    handlers::auth::{
        jwt::{JwtApi, JwtApiError},
        WalletAuthenticator,
    },
    indexer_jrpc_impl::IndexerJsonRpcNetworkInterface,
    notify::Notify,
    services::{AccountMonitorHandle, TransactionServiceHandle, WalletEvent, WebauthnService},
};

#[derive(Debug, Clone)]
pub struct HandlerContext {
    wallet_sdk: WalletSdk<SqliteWalletStore, IndexerJsonRpcNetworkInterface>,
    notifier: Notify<WalletEvent>,
    transaction_service: TransactionServiceHandle,
    account_monitor: AccountMonitorHandle,
    config: WalletDaemonConfig,
    authenticator: WalletAuthenticator,
    shutdown_signal: ShutdownSignal,
}

impl HandlerContext {
    pub fn new(
        wallet_sdk: WalletSdk<SqliteWalletStore, IndexerJsonRpcNetworkInterface>,
        notifier: Notify<WalletEvent>,
        transaction_service: TransactionServiceHandle,
        account_monitor: AccountMonitorHandle,
        config: WalletDaemonConfig,
        authenticator: WalletAuthenticator,
        shutdown_signal: ShutdownSignal,
    ) -> Self {
        Self {
            wallet_sdk,
            notifier,
            transaction_service,
            account_monitor,
            config,
            authenticator,
            shutdown_signal,
        }
    }

    pub fn notifier(&self) -> &Notify<WalletEvent> {
        &self.notifier
    }

    pub fn wallet_sdk(&self) -> &WalletSdk<SqliteWalletStore, IndexerJsonRpcNetworkInterface> {
        &self.wallet_sdk
    }

    pub fn check_auth(&self, token: Option<&Bearer>, permissions: &[JrpcPermission]) -> Result<(), JwtApiError> {
        self.jwt_api().check_auth(token, permissions)
    }

    pub fn jwt_api(&self) -> JwtApi<'_, SqliteWalletStore> {
        JwtApi::new(
            self.wallet_sdk.store(),
            self.config().jwt_expiry,
            // TODO: these are the same
            self.config().jwt_secret_key.clone(),
            self.config().jwt_secret_key.clone(),
        )
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

    pub fn webauthn_service(&self) -> Option<&WebauthnService<SqliteWalletStore>> {
        self.authenticator.webauthn_service()
    }

    pub fn webauthn(&self) -> Option<&Webauthn> {
        self.authenticator.webauthn()
    }

    /// Returns a TransactionBuilder with the current network configured.
    pub fn transaction_builder(&self) -> TransactionBuilder {
        Transaction::builder().for_network(self.config().network.as_byte())
    }
}
