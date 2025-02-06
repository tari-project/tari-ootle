//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::handlers::auth::Authenticator;
use crate::services::WebauthnService;
use crate::{
    config::WalletDaemonConfig,
    indexer_jrpc_impl::IndexerJsonRpcNetworkInterface,
    notify::Notify,
    services::{AccountMonitorHandle, TransactionServiceHandle, WalletEvent},
};
use std::sync::Arc;
use tari_dan_wallet_sdk::DanWalletSdk;
use tari_dan_wallet_storage_sqlite::SqliteWalletStore;
use webauthn_rs::Webauthn;

#[derive(Debug, Clone)]
pub struct HandlerContext {
    wallet_sdk: DanWalletSdk<SqliteWalletStore, IndexerJsonRpcNetworkInterface>,
    notifier: Notify<WalletEvent>,
    transaction_service: TransactionServiceHandle,
    account_monitor: AccountMonitorHandle,
    config: WalletDaemonConfig,
    webauthn: Option<Webauthn>,
    authenticator: Arc<dyn Authenticator>,
    webauthn_service: Arc<WebauthnService<SqliteWalletStore>>,
}

impl HandlerContext {
    pub fn new(
        wallet_sdk: DanWalletSdk<SqliteWalletStore, IndexerJsonRpcNetworkInterface>,
        notifier: Notify<WalletEvent>,
        transaction_service: TransactionServiceHandle,
        account_monitor: AccountMonitorHandle,
        config: WalletDaemonConfig,
        authenticator: Arc<dyn Authenticator>,
        webauthn_service: Arc<WebauthnService<SqliteWalletStore>>,
        webauthn: Option<Webauthn>,
    ) -> Self {
        Self {
            wallet_sdk,
            notifier,
            transaction_service,
            account_monitor,
            config,
            webauthn,
            authenticator,
            webauthn_service,
        }
    }

    pub fn notifier(&self) -> &Notify<WalletEvent> {
        &self.notifier
    }

    pub fn wallet_sdk(&self) -> &DanWalletSdk<SqliteWalletStore, IndexerJsonRpcNetworkInterface> {
        &self.wallet_sdk
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

    pub fn authenticator(&self) -> Arc<dyn Authenticator> {
        self.authenticator.clone()
    }

    pub fn webauthn_service(&self) -> Arc<WebauthnService<SqliteWalletStore>> {
        self.webauthn_service.clone()
    }

    pub fn webauthn(&self) -> Option<&Webauthn> {
        self.webauthn.as_ref()
    }
}
