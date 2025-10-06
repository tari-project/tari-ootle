//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::Display,
    future::poll_fn,
    task::{ready, Context, Poll},
    time::Duration,
};

use futures_bounded::PushError;
use log::{info, warn};
use tari_ootle_common_types::optional::IsNotFoundError;
use tari_ootle_wallet_sdk::{
    network::{StatusResponseError, WalletNetworkInterface},
    storage::WalletStore,
    WalletSdk,
};
use tari_template_lib::{models::ComponentAddress, prelude::ResourceAddress};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

use crate::utxo_scanner::UtxoScanner;

const LOG_TARGET: &str = "tari::ootle::wallet_services::stealth_utxo_scanner";

const MAX_CONCURRENT_SCANS: usize = 10;

#[derive(Debug, Clone)]
pub struct UtxoScannerHandle {
    tx: mpsc::UnboundedSender<UtxoScanRequest>,
    notify_sub: watch::Receiver<()>,
}

impl UtxoScannerHandle {
    pub fn request_scan(&self, account_address: ComponentAddress, resource_address: ResourceAddress) {
        if let Err(e) = self.tx.send(UtxoScanRequest {
            account_address,
            resource_address,
        }) {
            warn!(target: LOG_TARGET, "❓️ NEVER HAPPEN: UTXO scan request channel disconnected: {}", e);
        }
    }

    pub fn subscribe_notifications(&self) -> watch::Receiver<()> {
        self.notify_sub.clone()
    }
}

pub struct StealthUtxoScannerWorker<TStore, TNetworkInterface> {
    scanner: StealthUtxoScanner<TStore, TNetworkInterface>,
}

impl<TStore, TNetworkInterface> StealthUtxoScannerWorker<TStore, TNetworkInterface>
where
    TStore: WalletStore + Clone + Send + Sync + 'static,
    TNetworkInterface: WalletNetworkInterface + Clone + Send + Sync + 'static,
    TNetworkInterface::Error: IsNotFoundError + StatusResponseError,
{
    pub fn new(sdk: WalletSdk<TStore, TNetworkInterface>) -> Self {
        Self {
            scanner: StealthUtxoScanner::new(sdk.clone()),
        }
    }

    pub fn spawn(self) -> (JoinHandle<anyhow::Result<()>>, UtxoScannerHandle) {
        let (tx, rx) = mpsc::unbounded_channel();
        let notify_sub = self.scanner.subscribe_notifications();

        let handle = tokio::spawn(async move {
            let mut worker = self;
            worker.run(rx).await;
            Ok(())
        });

        (handle, UtxoScannerHandle { tx, notify_sub })
    }

    async fn run(&mut self, mut work_queue: mpsc::UnboundedReceiver<UtxoScanRequest>) {
        info!(target: LOG_TARGET, "🔍️ Stealth UTXO scanner worker started");
        loop {
            let poll_fut = poll_fn(|cx| self.scanner.poll(cx));
            tokio::select! {
                biased;
                maybe_req = work_queue.recv() => {
                    match maybe_req {
                        Some(req) => {
                            self.scanner.enqueue_work(req);
                        },
                        None => break, // Channel closed
                    };
                },
                _ = poll_fut => {
                    // All work completed - continue
                },
            }
        }

        info!(target: LOG_TARGET, "🔍️ Stealth UTXO scanner worker exiting");
    }
}

type ScanResult = anyhow::Result<()>;

pub struct StealthUtxoScanner<TStore, TNetworkInterface> {
    in_progress_work: futures_bounded::FuturesMap<UtxoScanRequest, ScanResult>,
    sdk: WalletSdk<TStore, TNetworkInterface>,
    notify_tx: watch::Sender<()>,
}

impl<TStore, TNetworkInterface> StealthUtxoScanner<TStore, TNetworkInterface>
where
    TStore: WalletStore + Clone + Send + Sync + 'static,
    TNetworkInterface: WalletNetworkInterface + Clone + Send + Sync + 'static,
    TNetworkInterface::Error: IsNotFoundError + StatusResponseError,
{
    pub(self) fn new(sdk: WalletSdk<TStore, TNetworkInterface>) -> Self {
        let (notify_tx, _) = watch::channel::<()>(());
        Self {
            in_progress_work: futures_bounded::FuturesMap::new(Duration::from_secs(300), MAX_CONCURRENT_SCANS),
            sdk,
            notify_tx,
        }
    }

    pub fn subscribe_notifications(&self) -> watch::Receiver<()> {
        self.notify_tx.subscribe()
    }

    pub(self) fn enqueue_work(&mut self, task: UtxoScanRequest) {
        info!(target: LOG_TARGET, "🔍️ Received scan request for {}", task);

        if self.in_progress_work.contains(task) {
            info!(target: LOG_TARGET, "🔍️ Scan for {} is already in progress, ignoring request", task);
            return;
        }
        match self
            .in_progress_work
            .try_push(task, do_work(self.sdk.clone(), self.notify_tx.clone(), task))
        {
            Ok(()) => {},
            Err(PushError::BeyondCapacity(_)) => {
                warn!(
                    target: LOG_TARGET,
                    "Cannot queue scan for {}: maximum concurrent scans reached",
                    task
                );
            },
            Err(PushError::Replaced(_)) => {
                unreachable!("BUG: Already checked for existing work but got Replaced error")
            },
        }
    }

    pub(self) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let (task, result) = ready!(self.in_progress_work.poll_unpin(cx));
        match result {
            Ok(Ok(_)) => {
                info!(target: LOG_TARGET, "🔍️ Completed scan for {}", task);
            },
            Ok(Err(e)) => {
                warn!(target: LOG_TARGET, "❓️ Error during UTXO scan for {}: {}", task, e);
            },
            Err(_) => {
                warn!(target: LOG_TARGET, "❓️ UTXO scan for {} timed out", task);
            },
        }
        // NOTE: do not return Ready here. The caller is polling in a loop, and if there is no work to do, the loop will
        // spin.
        Poll::Pending
    }
}

async fn do_work<TStore, TNetworkInterface>(
    sdk: WalletSdk<TStore, TNetworkInterface>,
    notify_tx: watch::Sender<()>,
    task: UtxoScanRequest,
) -> ScanResult
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError + StatusResponseError,
{
    info!(target: LOG_TARGET, "🔍 Scanning for UTXOs for {}", task);
    let account = sdk.accounts_api().get_account_by_address(&task.account_address)?;
    let num_found = UtxoScanner::new(sdk)
        .scan_and_enqueue_utxos(&account, &task.resource_address)
        .await?;

    // UTXOs were found, notify the Utxo recovery worker that there is work to do
    if num_found > 0 {
        let _ = notify_tx.send(());
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct UtxoScanRequest {
    account_address: ComponentAddress,
    resource_address: ResourceAddress,
}

impl Display for UtxoScanRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.account_address, self.resource_address)
    }
}
