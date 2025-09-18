//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod events;
pub use events::*;

mod account_monitor;
pub use account_monitor::AccountMonitorHandle;

pub mod transaction_service;
mod webauthn;
pub use webauthn::*;

mod session_store;
mod template_monitor;

pub mod recovery_service;
mod stealth_utxo_scanner;

// -------------------------------- Spawn -------------------------------- //
use anyhow::anyhow;
use futures::{future, future::BoxFuture, FutureExt};
pub use session_store::*;
use tari_ootle_common_types::optional::IsNotFoundError;
use tari_ootle_wallet_sdk::{
    network::{StatusResponseError, WalletNetworkInterface},
    storage::WalletStore,
    WalletSdk,
};
use tari_ootle_wallet_sdk_services::utxo_scanner::UtxoRecovery;
use tari_shutdown::ShutdownSignal;
use tokio::{sync::oneshot, task::JoinHandle};
use transaction_service::TransactionService;
pub use transaction_service::TransactionServiceHandle;

use crate::{
    notify::Notify,
    services::{
        account_monitor::AccountMonitor,
        stealth_utxo_scanner::StealthUtxoScannerWorker,
        template_monitor::TemplateMonitor,
    },
};

type Reply<T> = oneshot::Sender<T>;

pub fn spawn_services<TStore, TNetworkInterface>(
    shutdown_signal: ShutdownSignal,
    notify: Notify<WalletEvent>,
    wallet_sdk: WalletSdk<TStore, TNetworkInterface>,
) -> Services
where
    TStore: WalletStore + Clone + Send + Sync + 'static,
    TNetworkInterface: WalletNetworkInterface + Clone + Send + Sync + 'static,
    TNetworkInterface::Error: IsNotFoundError + StatusResponseError,
{
    let (transaction_service, transaction_service_handle) =
        TransactionService::new(notify.clone(), wallet_sdk.clone(), shutdown_signal.clone());
    let transaction_service_join_handle = tokio::spawn(transaction_service.run());

    let template_monitor = TemplateMonitor::new(notify.clone(), wallet_sdk.clone(), shutdown_signal.clone());
    let template_monitor_join_handle = tokio::spawn(template_monitor.run());

    let utxo_scanner = StealthUtxoScannerWorker::new(wallet_sdk.clone());
    let (utxo_scanner_join_handle, utxo_scanner_handle) = utxo_scanner.spawn();

    let utxo_recovery_join_handle = {
        let sdk = wallet_sdk.clone();
        let notify_sub = utxo_scanner_handle.subscribe_notifications();
        tokio::spawn(UtxoRecovery::new(sdk).run(notify_sub))
    };

    let (account_monitor, account_monitor_handle) =
        AccountMonitor::new(notify, wallet_sdk, utxo_scanner_handle, shutdown_signal);
    let account_monitor_join_handle = tokio::spawn(account_monitor.run());

    Services {
        account_monitor_handle,
        transaction_service_handle,
        services_fut: try_select_any([
            transaction_service_join_handle,
            account_monitor_join_handle,
            template_monitor_join_handle,
            utxo_scanner_join_handle,
            utxo_recovery_join_handle,
        ])
        .boxed(),
    }
}

pub struct Services {
    pub services_fut: BoxFuture<'static, Result<(), anyhow::Error>>,
    pub account_monitor_handle: AccountMonitorHandle,
    pub transaction_service_handle: TransactionServiceHandle,
}

async fn try_select_any<I>(handles: I) -> Result<(), anyhow::Error>
where I: IntoIterator<Item = JoinHandle<Result<(), anyhow::Error>>> {
    let (res, _, _) = future::select_all(handles).await;
    match res {
        Ok(res) => res,
        Err(e) => Err(anyhow!("Task panicked: {}", e)),
    }
}
