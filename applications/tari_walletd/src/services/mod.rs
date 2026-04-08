//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub use refresh_token_store::*;
pub use webauthn_session_store::*;

pub mod wasm_optimizer;

mod auto_claim_burn_service;
mod claim_burn_monitor;
mod webauthn;
pub use webauthn::*;

mod refresh_token_store;
mod template_monitor;
mod webauthn_session_store;

// -------------------------------- Spawn -------------------------------- //
use std::path::PathBuf;

use anyhow::anyhow;
use futures::{FutureExt, future, future::BoxFuture};
use tari_ootle_wallet_sdk::{WalletSdk, models::WalletEvent};
use tari_ootle_wallet_sdk_services::{
    account_monitor::{AccountMonitor, AccountMonitorHandle},
    notify::Notify,
    transaction_service::{TransactionService, TransactionServiceHandle},
    utxo_scanner::{StealthUtxoScannerWorker, UtxoRecovery},
};
use tari_shutdown::ShutdownSignal;
use tokio::task::JoinHandle;

use crate::{
    OotleWalletDaemonSpec,
    services::{
        auto_claim_burn_service::AutoClaimBurnService,
        claim_burn_monitor::ClaimBurnMonitor,
        template_monitor::TemplateMonitor,
    },
};

pub fn spawn_services(
    shutdown_signal: ShutdownSignal,
    notify: Notify<WalletEvent>,
    wallet_sdk: WalletSdk<OotleWalletDaemonSpec>,
    burn_proof_dir: PathBuf,
    auto_claim_burns: bool,
) -> Services {
    let (transaction_service, transaction_service_handle) =
        TransactionService::new(notify.clone(), wallet_sdk.clone(), shutdown_signal.clone());
    let transaction_service_join_handle = tokio::spawn(transaction_service.run());

    let template_monitor = TemplateMonitor::new(notify.clone(), wallet_sdk.clone(), shutdown_signal.clone());
    let template_monitor_join_handle = tokio::spawn(template_monitor.run());

    let claim_burn_monitor = ClaimBurnMonitor::new(notify.clone(), burn_proof_dir.clone(), shutdown_signal.clone());
    let claim_burn_monitor_join_handle = tokio::spawn(claim_burn_monitor.run());

    let utxo_scanner = StealthUtxoScannerWorker::new(wallet_sdk.clone(), notify.clone());
    let (utxo_scanner_join_handle, utxo_scanner_handle) = utxo_scanner.spawn();

    let utxo_recovery_join_handle = {
        let sdk = wallet_sdk.clone();
        let notify_sub = utxo_scanner_handle.subscribe_notifications();
        tokio::spawn(UtxoRecovery::new(sdk).with_notify(notify.clone()).run(notify_sub))
    };

    let (account_monitor, account_monitor_handle) =
        AccountMonitor::new(notify, wallet_sdk.clone(), utxo_scanner_handle, shutdown_signal.clone());
    let account_monitor_join_handle = tokio::spawn(account_monitor.run());

    let mut join_handles = vec![
        transaction_service_join_handle,
        account_monitor_join_handle,
        template_monitor_join_handle,
        claim_burn_monitor_join_handle,
        utxo_scanner_join_handle,
        utxo_recovery_join_handle,
    ];

    if auto_claim_burns {
        let auto_claim_service = AutoClaimBurnService::new(
            wallet_sdk,
            transaction_service_handle.clone(),
            burn_proof_dir,
            shutdown_signal,
        );
        join_handles.push(tokio::spawn(auto_claim_service.run()));
    }

    Services {
        account_monitor_handle,
        transaction_service_handle,
        services_fut: try_select_any(join_handles).boxed(),
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
    res.unwrap_or_else(|e| Err(anyhow!("Task panicked: {}", e)))
}
