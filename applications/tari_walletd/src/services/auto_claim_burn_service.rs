//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, fs, path::PathBuf};

use anyhow::Context;
use log::*;
use notify::{
    Config,
    EventKind,
    RecommendedWatcher,
    RecursiveMode,
    Watcher,
    event::{AccessKind, AccessMode},
};
use tari_ootle_common_types::{Epoch, optional::Optional};
use tari_ootle_wallet_sdk::network::WalletNetworkInterface;
use tari_ootle_wallet_sdk_services::transaction_service::TransactionServiceHandle;
use tari_shutdown::ShutdownSignal;
use tari_sidechain::CompleteClaimBurnProof;
use tokio::{
    sync::mpsc,
    time::{Duration, MissedTickBehavior, interval},
};

use crate::{
    DEFAULT_FEE,
    WalletSdk,
    handlers::{accounts::execute_claim_burn, helpers::complete_burn_proof_to_contents},
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::auto_claim_burn";
const EPOCH_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Watches the burn proof directory for new JSON files and automatically submits claim burn
/// transactions on behalf of the wallet, deferring each claim until the next epoch as required
/// by the protocol.
///
/// ## Epoch-safety
/// A claim transaction submitted in the same epoch as the L1 burn will be rejected, because
/// L2 validators haven't yet committed that epoch's L1 state. This service records the current
/// epoch when a file is detected and only submits once the indexer reports a strictly later epoch.
///
/// ## Crash safety
/// Pending state is held in memory only. On restart the service re-scans the directory and
/// re-queues any unclaimed files with `Epoch(0)`, making them immediately eligible for
/// submission (the daemon restart itself implies enough time has passed). If a duplicate
/// submission occurs — because the previous claim was submitted but not yet finalized before
/// the restart — the second transaction will fail with "already claimed" on-chain. The
/// existing [`ClaimBurnMonitor`] handles that gracefully.
///
/// [`ClaimBurnMonitor`]: super::claim_burn_monitor::ClaimBurnMonitor
pub struct AutoClaimBurnService {
    sdk: WalletSdk,
    transaction_service: TransactionServiceHandle,
    burn_proof_dir: PathBuf,
    /// Maps file name → epoch at which the file was detected, or `None` if the epoch has not yet
    /// been determined (e.g. the indexer was unreachable when the file was first seen).
    /// `None` entries are resolved to `Some(current_epoch)` on the next successful interval check,
    /// after which the claim is submitted once the indexer reports a strictly later epoch.
    /// Startup-scan files use `Some(Epoch(0))` so they are eligible immediately.
    pending_claims: HashMap<String, Option<Epoch>>,
    shutdown_signal: ShutdownSignal,
}

impl AutoClaimBurnService {
    pub fn new(
        sdk: WalletSdk,
        transaction_service: TransactionServiceHandle,
        burn_proof_dir: PathBuf,
        shutdown_signal: ShutdownSignal,
    ) -> Self {
        Self {
            sdk,
            transaction_service,
            burn_proof_dir,
            pending_claims: HashMap::new(),
            shutdown_signal,
        }
    }

    pub async fn run(mut self) -> Result<(), anyhow::Error> {
        info!(
            target: LOG_TARGET,
            "🔥 Auto claim burn service started, watching {}",
            self.burn_proof_dir.display()
        );

        if let Err(e) = fs::create_dir_all(&self.burn_proof_dir) {
            warn!(target: LOG_TARGET, "Failed to create burn proof directory: {}", e);
        }

        // Start the watcher BEFORE scanning so files that arrive during the scan are not missed.
        let (event_tx, mut event_rx) = mpsc::channel::<notify::Result<notify::Event>>(100);
        let mut watcher = RecommendedWatcher::new(
            move |res| {
                // The watcher callback runs on the watcher's own thread, so blocking_send is correct here.
                let _unused = event_tx.blocking_send(res);
            },
            Config::default(),
        )?;
        watcher.watch(&self.burn_proof_dir, RecursiveMode::NonRecursive)?;

        // Recover any files that were present before this service started (e.g. placed while the
        // daemon was offline). Using Epoch(0) makes them immediately eligible — the daemon restart
        // itself implies the required epoch boundary has been crossed.
        self.scan_proof_dir();

        let mut epoch_check_interval = interval(EPOCH_CHECK_INTERVAL);
        epoch_check_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // Consume the immediate first tick so the first real check happens after EPOCH_CHECK_INTERVAL.
        epoch_check_interval.tick().await;

        loop {
            tokio::select! {
                _ = self.shutdown_signal.wait() => {
                    info!(target: LOG_TARGET, "🔥 Auto claim burn service shutting down");
                    break Ok(());
                }
                Some(res) = event_rx.recv() => {
                    self.on_fs_event(res).await;
                }
                _ = epoch_check_interval.tick() => {
                    self.check_and_submit_pending().await;
                }
            }
        }
        // watcher is dropped here, which stops the OS-level file watch.
    }

    /// Scans `burn_proof_dir` for any `.json` files present at startup and queues them with
    /// `Some(Epoch(0))` so they are submitted on the very next epoch check.
    fn scan_proof_dir(&mut self) {
        let entries = match fs::read_dir(&self.burn_proof_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                warn!(target: LOG_TARGET, "Failed to read burn proof directory: {}", e);
                return;
            },
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() &&
                path.extension().is_some_and(|ext| ext == "json") &&
                let Some(file_name) = path.file_name().and_then(|n| n.to_str())
            {
                info!(target: LOG_TARGET, "Found existing unclaimed burn proof: {}", file_name);
                // Epoch(0) makes startup files immediately eligible — the daemon restart itself
                // implies the required epoch boundary has been crossed.
                self.pending_claims.insert(file_name.to_string(), Some(Epoch(0)));
            }
        }

        if !self.pending_claims.is_empty() {
            info!(
                target: LOG_TARGET,
                "Startup scan queued {} unclaimed burn proof(s)",
                self.pending_claims.len()
            );
        }
    }

    async fn on_fs_event(&mut self, res: notify::Result<notify::Event>) {
        let event = match res {
            Ok(e) => e,
            Err(e) => {
                warn!(target: LOG_TARGET, "File watch error: {}", e);
                return;
            },
        };

        // React to:
        //  • Access(Close(Write))  — file fully written and closed (most reliable on Linux)
        //  • Create(_)             — file created (covers macOS FSEvents and Windows)
        //  • Modify(Name(To))      — atomic rename-into-directory (common for temp-file writes)
        let is_relevant = matches!(
            event.kind,
            EventKind::Access(AccessKind::Close(AccessMode::Write)) |
                EventKind::Create(_) |
                EventKind::Modify(notify::event::ModifyKind::Name(notify::event::RenameMode::To))
        );
        if !is_relevant {
            return;
        }

        for path in &event.paths {
            // Ignore anything outside the root of the burn_proof_dir (e.g. claimed/ or pending/)
            if path.parent() != Some(self.burn_proof_dir.as_path()) {
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if self.pending_claims.contains_key(file_name) {
                // Already queued (e.g. duplicate event for the same file).
                continue;
            }

            // Reserve the slot with None *before* the await point so that a second OS event for
            // the same file (notify sometimes fires Create + Access(Close(Write)) both) cannot
            // pass the `contains_key` check while we are suspended in `query_current_epoch`.
            self.pending_claims.insert(file_name.to_string(), None);

            let detected_epoch = match self.query_current_epoch().await {
                Ok(epoch) => epoch,
                Err(e) => {
                    warn!(
                        target: LOG_TARGET,
                        "Could not query epoch for newly detected file '{}' ({}). \
                         Epoch will be resolved on the next interval check.",
                        file_name,
                        e
                    );
                    // Leave the entry as None; check_and_submit_pending will resolve it
                    // once the indexer is reachable, then wait one full epoch before submitting.
                    continue;
                },
            };

            info!(
                target: LOG_TARGET,
                "New burn proof detected: '{}' at epoch {}. Will claim in epoch {}.",
                file_name,
                detected_epoch,
                detected_epoch.0.saturating_add(1),
            );
            self.pending_claims.insert(file_name.to_string(), Some(detected_epoch));
        }
    }

    /// For each queued claim whose detected epoch is strictly less than the current epoch,
    /// attempts to submit the claim transaction.
    async fn check_and_submit_pending(&mut self) {
        if self.pending_claims.is_empty() {
            return;
        }

        let current_epoch = match self.query_current_epoch().await {
            Ok(e) => e,
            Err(e) => {
                warn!(
                    target: LOG_TARGET,
                    "Failed to query current epoch, skipping claim check: {}",
                    e
                );
                return;
            },
        };

        // Resolve any entries where epoch detection previously failed (e.g. indexer was down).
        // They are stamped with the current epoch now and will be submitted one epoch later.
        for epoch_slot in self.pending_claims.values_mut() {
            if epoch_slot.is_none() {
                info!(
                    target: LOG_TARGET,
                    "Resolved pending epoch detection to epoch {}. Will claim in epoch {}.",
                    current_epoch,
                    current_epoch.0.saturating_add(1),
                );
                *epoch_slot = Some(current_epoch);
            }
        }

        let ready: Vec<String> = self
            .pending_claims
            .iter()
            .filter(|&(_, detected_epoch)| detected_epoch.is_some_and(|e| current_epoch > e))
            .map(|(name, _)| name.clone())
            .collect();

        for file_name in ready {
            match self.try_submit_claim(&file_name).await {
                Ok(tx_id) => {
                    info!(
                        target: LOG_TARGET,
                        "✅ Auto-submitted claim burn for '{}' (tx_id: {})",
                        file_name,
                        tx_id,
                    );
                    self.pending_claims.remove(&file_name);
                },
                Err(ClaimError::Permanent(e)) => {
                    error!(
                        target: LOG_TARGET,
                        "Permanent error auto-claiming '{}', removing from queue: {}. \
                         The file remains in the burn proof directory for manual retry.",
                        file_name,
                        e
                    );
                    self.pending_claims.remove(&file_name);
                },
                Err(ClaimError::Transient(e)) => {
                    warn!(
                        target: LOG_TARGET,
                        "Transient error auto-claiming '{}', will retry next interval: {}",
                        file_name,
                        e
                    );
                },
            }
        }
    }

    async fn try_submit_claim(&self, file_name: &str) -> Result<tari_ootle_transaction::TransactionId, ClaimError> {
        // Read and parse the proof file.
        let path = self.burn_proof_dir.join(file_name);
        let file = fs::File::open(&path)
            .with_context(|| format!("Failed to open burn proof file: {}", path.display()))
            .map_err(ClaimError::Permanent)?;
        let complete_proof: CompleteClaimBurnProof = serde_json::from_reader(&file)
            .with_context(|| format!("Failed to parse burn proof file: {}", path.display()))
            .map_err(ClaimError::Permanent)?;

        let proof_contents = complete_burn_proof_to_contents(complete_proof).map_err(ClaimError::Permanent)?;

        // Find the target account using the burn_public_key embedded in the proof.
        // This is the account whose owner key was used when burning on L1.
        let accounts_api = self.sdk.accounts_api();
        let account = accounts_api
            .get_account_by_public_key(&proof_contents.claim_proof.burn_public_key)
            .optional()
            .map_err(|e| ClaimError::Transient(e.into()))?
            .ok_or_else(|| {
                ClaimError::Permanent(anyhow::anyhow!(
                    "No account found for burn_public_key '{}'. The burn was not destined for any account in this \
                     wallet.",
                    proof_contents.claim_proof.burn_public_key,
                ))
            })?;

        // Delegate all crypto work and submission to the shared handler function.
        // Any error here is treated as permanent: by this point the file is readable and the account
        // is known, so failures indicate a bad proof (wrong key, ownership check failed, fee too high,
        // corrupt encrypted data). Retrying would produce the same result.
        let resp = execute_claim_burn(
            &self.sdk,
            &self.transaction_service,
            &account,
            proof_contents,
            DEFAULT_FEE,
            false,
            Some(file_name.to_string()),
        )
        .await
        .map_err(ClaimError::Permanent)?;

        Ok(resp.transaction_id)
    }

    async fn query_current_epoch(&self) -> anyhow::Result<Epoch> {
        self.sdk
            .get_network_interface()
            .get_current_epoch()
            .await
            .map_err(Into::into)
    }
}

/// Categorises errors to determine retry behaviour for auto-claims.
enum ClaimError {
    /// A permanent error (corrupt file, account not in this wallet). Remove from queue; the
    /// proof file remains in `burn_proof_dir` for the user to inspect and retry manually.
    Permanent(anyhow::Error),
    /// A transient error (network unavailable, indexer unreachable). Leave in queue and retry
    /// on the next epoch check interval.
    Transient(anyhow::Error),
}
