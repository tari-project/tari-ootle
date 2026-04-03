//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, fs, path::PathBuf};

use log::*;
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::models::{TransactionContext, WalletEvent};
use tari_ootle_wallet_sdk_services::notify::Notify;
use tari_shutdown::ShutdownSignal;
use tokio::sync::broadcast;

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::claim_burn_monitor";

pub struct ClaimBurnMonitor {
    notify_subscription: broadcast::Receiver<WalletEvent>,
    burn_proof_dir: PathBuf,
    pending_claims: HashMap<TransactionId, String>,
    shutdown_signal: ShutdownSignal,
}

impl ClaimBurnMonitor {
    pub fn new(notify: Notify<WalletEvent>, burn_proof_dir: PathBuf, shutdown_signal: ShutdownSignal) -> Self {
        Self {
            notify_subscription: notify.subscribe(),
            burn_proof_dir,
            pending_claims: HashMap::new(),
            shutdown_signal,
        }
    }

    pub async fn run(mut self) -> Result<(), anyhow::Error> {
        info!(target: LOG_TARGET, "🔥 Claim burn monitor started");

        loop {
            tokio::select! {
                _ = self.shutdown_signal.wait() => {
                    info!(target: LOG_TARGET, "🔥 Claim burn monitor shutting down");
                    break Ok(());
                }

                Ok(event) = self.notify_subscription.recv() => {
                    self.on_event(event);
                },
            }
        }
    }

    fn on_event(&mut self, event: WalletEvent) {
        match event {
            WalletEvent::TransactionSubmitted(event) => {
                if let Some(TransactionContext::ClaimBurn { file_name }) = event.context {
                    info!(
                        target: LOG_TARGET,
                        "Tracking claim burn transaction {} for proof file {}",
                        event.transaction_id,
                        file_name,
                    );
                    self.pending_claims.insert(event.transaction_id, file_name);
                }
            },
            WalletEvent::TransactionFinalized(event) => {
                if let Some(file_name) = self.pending_claims.remove(&event.transaction_id) {
                    if event.finalize.result.any_accept().is_some() {
                        info!(
                            target: LOG_TARGET,
                            "Claim burn transaction {} accepted, marking proof {} as claimed",
                            event.transaction_id,
                            file_name,
                        );
                        self.mark_as_claimed(&file_name);
                    } else {
                        warn!(
                            target: LOG_TARGET,
                            "Claim burn transaction {} was not accepted. Proof file {} remains available for retry.",
                            event.transaction_id,
                            file_name,
                        );
                    }
                }
            },
            WalletEvent::TransactionInvalid(event) => {
                if let Some(file_name) = self.pending_claims.remove(&event.transaction_id) {
                    warn!(
                        target: LOG_TARGET,
                        "Claim burn transaction {} is invalid ({}). Proof file {} remains available for retry.",
                        event.transaction_id,
                        event.status,
                        file_name,
                    );
                }
            },
            _ => {},
        }
    }

    fn mark_as_claimed(&self, file_name: &str) {
        let claimed_dir = self.burn_proof_dir.join("claimed");
        if let Err(e) = fs::create_dir_all(&claimed_dir) {
            warn!(
                target: LOG_TARGET,
                "Failed to create claimed directory {}: {}",
                claimed_dir.display(),
                e
            );
            return;
        }

        let src = self.burn_proof_dir.join(file_name);
        let dst = claimed_dir.join(file_name);
        if let Err(e) = fs::rename(&src, &dst) {
            warn!(
                target: LOG_TARGET,
                "Failed to move claimed burn proof {} -> {}: {}",
                src.display(),
                dst.display(),
                e
            );
        } else {
            info!(
                target: LOG_TARGET,
                "Burn proof {} marked as claimed",
                file_name
            );
        }
    }
}
