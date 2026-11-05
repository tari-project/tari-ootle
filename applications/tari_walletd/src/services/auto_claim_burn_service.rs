//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, path::PathBuf, time::Instant};

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
use ootle_byte_type::ToByteType;
use tari_engine_types::commit_result::{ExecutionFailureCode, FinalizeResult, RejectReason};
use tari_ootle_common_types::{Epoch, optional::Optional};
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::{
    models::{TransactionStatus, WalletEvent, WalletTransaction},
    network::WalletNetworkInterface,
};
use tari_ootle_wallet_sdk_services::{notify::Notify, transaction_service::TransactionServiceHandle};
use tari_shutdown::ShutdownSignal;
use tari_sidechain::CompleteClaimBurnProof;
use tokio::{
    sync::{broadcast, mpsc},
    time,
    time::{Duration, MissedTickBehavior},
};

use crate::{
    WalletSdk,
    handlers::{
        accounts::{ClaimBurnError, execute_claim_burn},
        helpers::complete_burn_proof_to_contents,
    },
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::auto_claim_burn";
const EPOCH_CHECK_INTERVAL: Duration = Duration::from_secs(30);
/// Validity window for an unattended claim: a few epochs is ample for the submit itself, and a
/// claim that does not land in that time is retried with a fresh window.
const CLAIM_TRANSACTION_VALIDITY_EPOCHS: u64 = 3;
/// Maximum retries for a failed wallet database read.
const MAX_RETRIES_STORE: u32 = 10;
/// Maximum retries for file read/parse errors (file still being written on macOS).
const MAX_RETRIES_FILE_READ: u32 = 1;
/// Longest wait between attempts at a claim the network has not taken.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(10 * 60);

/// True if a rejection indicates the burn is valid but its L1 block is not yet synced into a
/// claimable epoch, as opposed to a genuinely invalid claim.
///
/// The engine says which of the two it is: `TariClaimBurnProofVerifier` returns
/// `ClaimProofRejection::NotYetValid` for a header from the current epoch or one it has not synced, and that
/// reaches here as [`ExecutionFailureCode::NotYetValid`].
fn is_burn_not_yet_claimable(reject_reason: &RejectReason) -> bool {
    reject_reason.execution_failure_code() == Some(ExecutionFailureCode::NotYetValid)
}

/// Watches the burn proof directory for new JSON files and automatically submits claim burn
/// transactions on behalf of the wallet, deferring each claim until the burn's L1 block is
/// claimable on L2.
///
/// ## Epoch-safety
/// A claim is only valid once L2 validators have synced the L1 block containing the burn into a
/// committed epoch. That sync lags the L1 tip by `base_layer_confirmations` (e.g. 780 blocks on
/// mainnet), so submitting too early is rejected. Each proof file records the L1 epoch the burn was
/// mined in (`mined_in_epoch`); this service reads it and only submits once the network reports a
/// strictly later epoch. A claim the dry run or the network still finds not yet claimable is queued
/// again. A submitted claim stays queued until its transaction is finalized, since only the network's
/// result says whether it landed or must wait for a later epoch.
///
/// ## Network availability
/// A burn proof does not expire, so a claim the network has not taken stays queued for as long as the
/// daemon runs: while the indexer is unreachable no claim is attempted, and a claim that fails on a
/// network read or submission, or is not yet claimable, is attempted again after a delay that doubles
/// up to `MAX_RETRY_DELAY`. A claim leaves the queue only once it is accepted, the network rejects
/// it, or its proof file cannot be read or claimed by this wallet.
///
/// ## Crash safety
/// Pending state is held in memory only. On restart the service re-scans the directory and
/// re-queues any unclaimed files; each has its claim epoch re-derived from the proof file, so a
/// restart never submits a burn before it is claimable. If a duplicate submission occurs — because
/// the previous claim was submitted but not yet finalized before the restart — the second
/// transaction will fail with "already claimed" on-chain. The existing [`ClaimBurnMonitor`] handles
/// that gracefully.
///
/// [`ClaimBurnMonitor`]: super::claim_burn_monitor::ClaimBurnMonitor
pub struct AutoClaimBurnService {
    sdk: WalletSdk,
    transaction_service: TransactionServiceHandle,
    burn_proof_dir: PathBuf,
    /// The wallet's `max_fee_limit`: the most any claim this service submits may reveal.
    max_fee_limit: u64,
    /// Maps file name → pending claim state: the L1 epoch the burn must be past before claiming
    /// (resolved from the proof file) plus retry and back-off state.
    pending_claims: HashMap<String, PendingClaim>,
    wallet_events: broadcast::Receiver<WalletEvent>,
    shutdown_signal: ShutdownSignal,
}

impl AutoClaimBurnService {
    pub fn new(
        sdk: WalletSdk,
        transaction_service: TransactionServiceHandle,
        burn_proof_dir: PathBuf,
        max_fee_limit: u64,
        notify: &Notify<WalletEvent>,
        shutdown_signal: ShutdownSignal,
    ) -> Self {
        Self {
            sdk,
            transaction_service,
            burn_proof_dir,
            max_fee_limit,
            pending_claims: HashMap::new(),
            wallet_events: notify.subscribe(),
            shutdown_signal,
        }
    }

    pub async fn run(mut self) -> Result<(), anyhow::Error> {
        info!(
            target: LOG_TARGET,
            "🔥 Auto claim burn service started, watching {}",
            self.burn_proof_dir.display()
        );

        if let Err(e) = tokio::fs::create_dir_all(&self.burn_proof_dir).await {
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
        // daemon was offline). Each is re-queued unresolved; its claim epoch is read from the proof
        // on the next check, so a restart never submits a burn before it is claimable.
        self.scan_proof_dir().await;

        // First tick after EPOCH_CHECK_INTERVAL
        let mut epoch_check_interval =
            time::interval_at((Instant::now() + EPOCH_CHECK_INTERVAL).into(), EPOCH_CHECK_INTERVAL);
        epoch_check_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

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
                result = self.wallet_events.recv() => {
                    match result {
                        Ok(event) => self.on_wallet_event(event),
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(
                                target: LOG_TARGET,
                                "Missed {} wallet events; reading the result of each claim still awaiting one.",
                                n
                            );
                            self.refresh_in_flight(None);
                        },
                        Err(broadcast::error::RecvError::Closed) => {
                            info!(target: LOG_TARGET, "🔥 Wallet event channel closed, shutting down");
                            break Ok(());
                        },
                    }
                }
            }
        }
        // watcher is dropped here, which stops the OS-level file watch.
    }

    /// Scans `burn_proof_dir` for any `.json` files present at startup and queues them unresolved;
    /// each file's claim epoch is read from the proof itself on the next epoch check.
    async fn scan_proof_dir(&mut self) {
        let mut read_dir = match tokio::fs::read_dir(&self.burn_proof_dir).await {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                warn!(target: LOG_TARGET, "Failed to read burn proof directory: {}", e);
                return;
            },
        };

        loop {
            match read_dir.next_entry().await {
                Ok(Some(entry)) => {
                    let is_file = entry.file_type().await.map(|ft| ft.is_file()).unwrap_or(false);
                    if !is_file {
                        continue;
                    }
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "json") &&
                        let Some(file_name) = path.file_name().and_then(|n| n.to_str())
                    {
                        info!(target: LOG_TARGET, "Found existing unclaimed burn proof: {}", file_name);
                        self.pending_claims.insert(file_name.to_string(), PendingClaim::new());
                    }
                },
                Ok(None) => break,
                Err(e) => {
                    warn!(target: LOG_TARGET, "Failed to read directory entry: {}", e);
                },
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

            // Queue unresolved; the claim epoch is read from the proof file on the next interval
            // check (the file may still be mid-write at this point).
            info!(target: LOG_TARGET, "New burn proof detected: '{}'", file_name);
            self.pending_claims.insert(file_name.to_string(), PendingClaim::new());
        }
    }

    /// Resolves the claim epoch for any unresolved queued files by reading each proof file's
    /// `mined_in_epoch`: the burn is claimable once L2 is strictly past that epoch. Files that are not
    /// yet readable are left unresolved and retried next interval.
    async fn resolve_claim_epochs(&mut self) {
        let unresolved: Vec<String> = self
            .pending_claims
            .iter()
            .filter(|&(_, pending)| pending.claim_after_epoch.is_none())
            .map(|(name, _)| name.clone())
            .collect();
        for file_name in unresolved {
            let after = match self.read_proof_file(&file_name).await {
                Ok(proof) => {
                    let mined = proof.mined_in_epoch;
                    info!(
                        target: LOG_TARGET,
                        "Burn proof '{}' was mined in L1 epoch {}; will claim once L2 passes it (epoch {}).",
                        file_name,
                        mined,
                        mined.saturating_add(1),
                    );
                    Epoch(mined)
                },
                Err(e) => {
                    let pending = self.pending_claims.get_mut(&file_name).expect("just iterated");
                    pending.retries += 1;
                    let retries = pending.retries;
                    if retries >= MAX_RETRIES_FILE_READ {
                        error!(
                            target: LOG_TARGET,
                            "Failed to read burn proof '{}' to resolve its claim epoch after {} attempts: {}. \
                             Removing from queue; the file remains in the burn proof directory for a manual claim.",
                            file_name,
                            retries,
                            e,
                        );
                        self.pending_claims.remove(&file_name);
                    } else {
                        debug!(
                            target: LOG_TARGET,
                            "Could not read burn proof '{}' (attempt {}/{}): {}; will retry next interval.",
                            file_name,
                            retries,
                            MAX_RETRIES_FILE_READ,
                            e,
                        );
                    }
                    continue;
                },
            };
            if let Some(pending) = self.pending_claims.get_mut(&file_name) {
                pending.claim_after_epoch = Some(after);
            }
        }
    }

    /// Resolves epochs for newly queued files, then attempts to submit each claim whose target
    /// epoch is strictly less than the current epoch.
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

        self.refresh_in_flight(Some(current_epoch));
        self.resolve_claim_epochs().await;

        let now = Instant::now();
        let ready: Vec<String> = self
            .pending_claims
            .iter()
            .filter(|&(_, pending)| {
                pending.in_flight.is_none() &&
                    pending.is_due(now) &&
                    pending.claim_after_epoch.is_some_and(|e| current_epoch > e)
            })
            .map(|(name, _)| name.clone())
            .collect();

        for file_name in ready {
            match self.try_submit_claim(&file_name, current_epoch).await {
                Ok(tx_id) => {
                    info!(
                        target: LOG_TARGET,
                        "✅ Auto-submitted claim burn for '{}' (tx_id: {})",
                        file_name,
                        tx_id,
                    );
                    if let Some(pending) = self.pending_claims.get_mut(&file_name) {
                        pending.in_flight = Some(InFlightClaim {
                            transaction_id: tx_id,
                            max_epoch: claim_max_epoch(current_epoch),
                        });
                    }
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
                Err(ClaimError::Transient { error: e, max_retries }) => {
                    let pending = self.pending_claims.get_mut(&file_name).expect("just iterated");
                    pending.retries += 1;
                    if pending.retries >= max_retries {
                        error!(
                            target: LOG_TARGET,
                            "Giving up on '{}' after {} retries: {}. \
                             The file remains in the burn proof directory for manual retry.",
                            file_name,
                            pending.retries,
                            e,
                        );
                        self.pending_claims.remove(&file_name);
                    } else {
                        warn!(
                            target: LOG_TARGET,
                            "Transient error auto-claiming '{}' (attempt {}/{}), will retry next interval: {}",
                            file_name,
                            pending.retries,
                            max_retries,
                            e,
                        );
                    }
                },
                Err(ClaimError::Network(e)) => {
                    back_off_claim(&mut self.pending_claims, &file_name, now, BackOffCause::Network(&e));
                },
                Err(ClaimError::Deferred) => {
                    back_off_claim(&mut self.pending_claims, &file_name, now, BackOffCause::NotYetClaimable);
                },
            }
        }
    }

    fn on_wallet_event(&mut self, event: WalletEvent) {
        let (transaction_id, outcome) = match &event {
            WalletEvent::TransactionFinalized(event) => (event.transaction_id, finalized_outcome(&event.finalize)),
            WalletEvent::TransactionInvalid(event) => {
                (event.transaction_id, ClaimOutcome::Failed(event.status.to_string()))
            },
            _ => return,
        };
        settle_claim(&mut self.pending_claims, &transaction_id, outcome, Instant::now());
    }

    /// Settles each in-flight claim whose transaction the wallet has stored as final, for a result
    /// whose event may have been missed. One still pending once `current_epoch` is past its
    /// `max_epoch` can no longer be included, so the claim is submitted again.
    fn refresh_in_flight(&mut self, current_epoch: Option<Epoch>) {
        let in_flight: Vec<InFlightClaim> = self.pending_claims.values().filter_map(|p| p.in_flight).collect();
        for claim in in_flight {
            let transaction = match self.sdk.transaction_api().get(claim.transaction_id) {
                Ok(transaction) => transaction,
                Err(e) => {
                    debug!(
                        target: LOG_TARGET,
                        "Could not read claim burn transaction {}: {}",
                        claim.transaction_id,
                        e
                    );
                    continue;
                },
            };
            let outcome = match stored_outcome(&transaction) {
                Some(outcome) => outcome,
                None if current_epoch.is_some_and(|epoch| epoch > claim.max_epoch) => ClaimOutcome::Expired,
                None => continue,
            };
            settle_claim(&mut self.pending_claims, &claim.transaction_id, outcome, Instant::now());
        }
    }

    /// Reads and deserializes a burn proof file from `burn_proof_dir`.
    async fn read_proof_file(&self, file_name: &str) -> anyhow::Result<CompleteClaimBurnProof> {
        let path = self.burn_proof_dir.join(file_name);
        let file = tokio::fs::File::open(&path)
            .await
            .with_context(|| format!("Failed to open burn proof file: {}", path.display()))?;
        serde_json::from_reader(file.into_std().await)
            .with_context(|| format!("Failed to parse burn proof file: {}", path.display()))
    }

    async fn try_submit_claim(
        &self,
        file_name: &str,
        current_epoch: Epoch,
    ) -> Result<tari_ootle_transaction::TransactionId, ClaimError> {
        let max_epoch = claim_max_epoch(current_epoch);
        let complete_proof = self
            .read_proof_file(file_name)
            .await
            .map_err(|e| ClaimError::transient(e, MAX_RETRIES_FILE_READ))?;

        let burn_public_key = complete_proof.claim_proof.burn_public_key.to_byte_type();
        let proof_contents = complete_burn_proof_to_contents(complete_proof).map_err(ClaimError::Permanent)?;

        // Find the target account using the burn_public_key embedded in the proof.
        // This is the account whose owner key was used when burning on L1.
        let accounts_api = self.sdk.accounts_api();
        let account = accounts_api
            .get_account_by_public_key(&burn_public_key)
            .optional()
            .map_err(|e| ClaimError::transient(e.into(), MAX_RETRIES_STORE))?
            .ok_or_else(|| {
                ClaimError::Permanent(anyhow::anyhow!(
                    "No account found for burn_public_key '{}'. The burn was not destined for any account in this \
                     wallet.",
                    burn_public_key,
                ))
            })?;

        let resp = execute_claim_burn(
            &self.sdk,
            &self.transaction_service,
            &account,
            proof_contents.clone(),
            1,
            max_epoch,
            true,
            Some(file_name.to_string()),
        )
        .await
        .map_err(ClaimError::from)?;

        let Some(result) = resp.dry_run_result else {
            return Err(ClaimError::Permanent(anyhow::anyhow!(
                "NEVER HAPPEN: Dry run failed for '{}', cannot submit claim: no result returned for dry run.",
                file_name,
            )));
        };
        if let Some(reason) = result.finalize.any_reject() {
            // A burn whose L1 block validators have not yet synced is deferred so the claim lands
            // once the base layer catches up, rather than being dropped as permanent.
            if is_burn_not_yet_claimable(reason) {
                return Err(ClaimError::Deferred);
            }
            return Err(ClaimError::Permanent(anyhow::anyhow!(
                "Dry run rejected for '{}': {}. Cannot submit claim.",
                file_name,
                reason,
            )));
        }
        let Some(required_fees) = resp.required_fees else {
            return Err(ClaimError::Permanent(anyhow::anyhow!(
                "NEVER HAPPEN: Dry run failed for '{}', cannot submit claim: no required fees for dry run.",
                file_name,
            )));
        };

        // Delegate all crypto work and submission to the shared handler function. The dry run's fee is
        // the indexer's figure, so the handler treats it as a ceiling and reveals no more than its own
        // price for the claim. The wallet's `max_fee_limit` bounds that ceiling in turn.
        let max_fee = required_fees.min(self.max_fee_limit);
        let resp = execute_claim_burn(
            &self.sdk,
            &self.transaction_service,
            &account,
            proof_contents,
            max_fee,
            max_epoch,
            false,
            Some(file_name.to_string()),
        )
        .await
        .map_err(ClaimError::from)?;

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

/// The network's result for a submitted claim transaction.
enum ClaimOutcome {
    Accepted,
    /// Rejected because the burn is not claimable in the epoch the claim executed in.
    NotYetClaimable,
    Failed(String),
    /// Not finalized by its `max_epoch`, so it can no longer be included.
    Expired,
}

fn finalized_outcome(finalize: &FinalizeResult) -> ClaimOutcome {
    match finalize.any_reject() {
        None => ClaimOutcome::Accepted,
        Some(reason) if is_burn_not_yet_claimable(reason) => ClaimOutcome::NotYetClaimable,
        Some(reason) => ClaimOutcome::Failed(reason.to_string()),
    }
}

/// The outcome the wallet has stored for a claim transaction, or `None` while it is not final.
fn stored_outcome(transaction: &WalletTransaction) -> Option<ClaimOutcome> {
    match transaction.status {
        TransactionStatus::New | TransactionStatus::DryRun | TransactionStatus::Pending => None,
        TransactionStatus::Accepted | TransactionStatus::Rejected | TransactionStatus::OnlyFeeAccepted => {
            Some(match &transaction.finalize {
                Some(finalize) => finalized_outcome(finalize),
                None if transaction.status.is_accepted() => ClaimOutcome::Accepted,
                // A consensus abort is final with no execution result.
                None => ClaimOutcome::Failed(transaction.status.to_string()),
            })
        },
        TransactionStatus::InvalidTransaction => Some(ClaimOutcome::Failed(
            transaction
                .invalid_reason
                .clone()
                .unwrap_or_else(|| transaction.status.to_string()),
        )),
    }
}

/// Settles the claim awaiting `transaction_id`: an accepted claim leaves the queue (the
/// [`ClaimBurnMonitor`](super::claim_burn_monitor::ClaimBurnMonitor) moves its file), one that is not
/// yet claimable backs off to be submitted again, and any other is dropped.
fn settle_claim(
    pending_claims: &mut HashMap<String, PendingClaim>,
    transaction_id: &TransactionId,
    outcome: ClaimOutcome,
    now: Instant,
) {
    let Some(file_name) = pending_claims
        .iter()
        .find(|(_, pending)| pending.in_flight.is_some_and(|c| c.transaction_id == *transaction_id))
        .map(|(name, _)| name.clone())
    else {
        return;
    };
    match outcome {
        ClaimOutcome::Accepted => {
            info!(target: LOG_TARGET, "Claim burn for '{}' (tx_id: {}) was accepted", file_name, transaction_id);
            pending_claims.remove(&file_name);
        },
        ClaimOutcome::NotYetClaimable => {
            if let Some(pending) = pending_claims.get_mut(&file_name) {
                pending.in_flight = None;
            }
            back_off_claim(pending_claims, &file_name, now, BackOffCause::NotYetClaimable);
        },
        ClaimOutcome::Expired => {
            info!(
                target: LOG_TARGET,
                "Claim burn for '{}' (tx_id: {}) was not finalized by its max epoch; submitting it again",
                file_name,
                transaction_id,
            );
            if let Some(pending) = pending_claims.get_mut(&file_name) {
                pending.in_flight = None;
            }
        },
        ClaimOutcome::Failed(reason) => {
            warn!(
                target: LOG_TARGET,
                "Claim burn for '{}' (tx_id: {}) was not accepted: {}. The file remains in the burn proof directory \
                 for manual retry.",
                file_name,
                transaction_id,
                reason,
            );
            pending_claims.remove(&file_name);
        },
    }
}

/// Why a claim waits before it is attempted again.
enum BackOffCause<'a> {
    /// The burn's L1 block has not been synced by validators yet.
    NotYetClaimable,
    /// A network read or submission failed.
    Network(&'a anyhow::Error),
}

/// Keeps a claim the network has not taken queued, to be attempted again once its back-off delay
/// passes.
fn back_off_claim(
    pending_claims: &mut HashMap<String, PendingClaim>,
    file_name: &str,
    now: Instant,
    cause: BackOffCause<'_>,
) {
    let Some(pending) = pending_claims.get_mut(file_name) else {
        return;
    };
    let delay = pending.back_off(now);
    let attempts = pending.failed_attempts;
    match cause {
        BackOffCause::NotYetClaimable if attempts == 1 => info!(
            target: LOG_TARGET,
            "⏳ Burn claim '{}' is not yet claimable; its L1 burn block has not been synced by validators yet. \
             Will retry until it is.",
            file_name,
        ),
        BackOffCause::NotYetClaimable => debug!(
            target: LOG_TARGET,
            "Burn claim '{}' still not claimable (attempt {}), will retry in {:.0?}.",
            file_name,
            attempts,
            delay,
        ),
        BackOffCause::Network(error) => warn!(
            target: LOG_TARGET,
            "Network error auto-claiming '{}' (attempt {}), will retry in {:.0?}: {}",
            file_name,
            attempts,
            delay,
            error,
        ),
    }
}

/// How long a claim waits after its `failed_attempts`th consecutive failed attempt: none after the
/// first, so it is attempted at the next check, then doubling from `EPOCH_CHECK_INTERVAL` up to
/// `MAX_RETRY_DELAY`. The doubling bounds the dry runs a fleet of wallets sends a network that is
/// not yet taking claims.
fn retry_delay(failed_attempts: u32) -> Duration {
    match failed_attempts.checked_sub(2) {
        None => Duration::ZERO,
        Some(doublings) => EPOCH_CHECK_INTERVAL
            .saturating_mul(1u32.checked_shl(doublings).unwrap_or(u32::MAX))
            .min(MAX_RETRY_DELAY),
    }
}

struct PendingClaim {
    /// The L1 epoch the burn must be strictly past before it is claimable, read from the proof
    /// file's `mined_in_epoch`. `None` until resolved on an interval check; a proof lacking the
    /// field resolves to `Epoch(0)` (attempt immediately, deferring via the dry-run backstop).
    claim_after_epoch: Option<Epoch>,
    /// Number of transient errors encountered so far. The per-error `max_retries` limit determines
    /// when the claim is dropped from the queue (the file remains on disk for manual retry).
    retries: u32,
    /// Consecutive attempts the network did not take, because the burn was not yet claimable or a
    /// network read or submission failed. Sets the delay before `retry_at`.
    failed_attempts: u32,
    /// The claim is not attempted before this.
    retry_at: Option<Instant>,
    /// The submitted claim transaction whose result has not arrived yet. The claim is not submitted
    /// again while it is set.
    in_flight: Option<InFlightClaim>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InFlightClaim {
    transaction_id: TransactionId,
    max_epoch: Epoch,
}

impl PendingClaim {
    fn new() -> Self {
        Self {
            claim_after_epoch: None,
            retries: 0,
            failed_attempts: 0,
            retry_at: None,
            in_flight: None,
        }
    }

    fn is_due(&self, now: Instant) -> bool {
        self.retry_at.is_none_or(|retry_at| now >= retry_at)
    }

    /// Records a failed attempt and returns the delay before the next one.
    fn back_off(&mut self, now: Instant) -> Duration {
        self.failed_attempts = self.failed_attempts.saturating_add(1);
        let delay = retry_delay(self.failed_attempts);
        self.retry_at = Some(now + delay);
        delay
    }
}

/// The validity window stamped on claim transactions this service builds. Claims are submitted
/// unattended, so the window only has to cover the submission itself. Derived from the epoch the
/// caller already resolved: re-querying it here would turn a momentary indexer outage into a
/// permanent claim failure.
fn claim_max_epoch(current_epoch: Epoch) -> Epoch {
    Epoch(current_epoch.as_u64().saturating_add(CLAIM_TRANSACTION_VALIDITY_EPOCHS))
}

enum ClaimError {
    /// A permanent error (account not in this wallet, invalid proof data). Remove from queue; the
    /// proof file remains in `burn_proof_dir` for the user to inspect and retry manually.
    Permanent(anyhow::Error),
    /// A local failure that may clear (wallet database read, file still being written). Leave in
    /// queue and retry on the next epoch check interval, up to `max_retries` times.
    Transient { error: anyhow::Error, max_retries: u32 },
    /// A network read or submission failed (indexer unreachable or timed out, a substate served
    /// without a proof). Keep the claim queued and back off.
    Network(anyhow::Error),
    /// The burn is valid but not yet claimable: validators have not synced the L1 block containing
    /// the burn into an epoch at or before the current L2 epoch. Keep the claim queued and back off.
    Deferred,
}

impl ClaimError {
    fn transient(error: anyhow::Error, max_retries: u32) -> Self {
        Self::Transient { error, max_retries }
    }
}

impl From<ClaimBurnError> for ClaimError {
    fn from(error: ClaimBurnError) -> Self {
        match error {
            ClaimBurnError::Network(error) => Self::Network(error),
            ClaimBurnError::Build(error) => Self::Permanent(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_wallet_sdk::apis::substate::SubstateApiError;

    use super::*;

    #[test]
    fn a_network_failure_keeps_the_claim_queued() {
        let unproven = SubstateApiError::SubstateUnproven {
            substate_id: tari_template_lib_types::constants::TARI_TOKEN.into(),
        };
        assert!(matches!(
            ClaimError::from(ClaimBurnError::Network(unproven.into())),
            ClaimError::Network(_)
        ));
        assert!(matches!(
            ClaimError::from(ClaimBurnError::Build(anyhow::anyhow!("the claim proof is invalid"))),
            ClaimError::Permanent(_)
        ));
    }

    #[test]
    fn a_claim_stays_queued_however_long_the_network_does_not_take_it() {
        let mut queue = HashMap::from([("burn.json".to_string(), PendingClaim::new())]);
        let now = Instant::now();
        let error = anyhow::anyhow!("indexer unreachable");
        for attempt in 0..1000 {
            let cause = if attempt % 2 == 0 {
                BackOffCause::NotYetClaimable
            } else {
                BackOffCause::Network(&error)
            };
            back_off_claim(&mut queue, "burn.json", now, cause);
        }
        let pending = &queue["burn.json"];
        assert_eq!(pending.failed_attempts, 1000);
        assert!(!pending.is_due(now));
        assert!(pending.is_due(now + MAX_RETRY_DELAY));
    }

    #[test]
    fn retry_delay_doubles_from_the_next_check_up_to_the_cap() {
        assert_eq!(retry_delay(1), Duration::ZERO);
        assert_eq!(retry_delay(2), EPOCH_CHECK_INTERVAL);
        assert_eq!(retry_delay(3), EPOCH_CHECK_INTERVAL * 2);
        assert_eq!(retry_delay(4), EPOCH_CHECK_INTERVAL * 4);
        assert_eq!(retry_delay(100), MAX_RETRY_DELAY);
        assert_eq!(retry_delay(u32::MAX), MAX_RETRY_DELAY);
    }

    fn execution_failure(code: ExecutionFailureCode) -> RejectReason {
        RejectReason::ExecutionFailure {
            code,
            message: "At instruction #0: irrelevant to the decision".to_string(),
        }
    }

    fn queue_with_claim_in_flight(transaction_id: TransactionId) -> HashMap<String, PendingClaim> {
        let mut pending = PendingClaim::new();
        pending.claim_after_epoch = Some(Epoch(10));
        pending.in_flight = Some(InFlightClaim {
            transaction_id,
            max_epoch: Epoch(13),
        });
        HashMap::from([("burn.json".to_string(), pending)])
    }

    #[test]
    fn a_claim_rejected_as_not_yet_claimable_is_submitted_again() {
        let transaction_id = TransactionId::new([1; 32]);
        let mut queue = queue_with_claim_in_flight(transaction_id);
        let now = Instant::now();
        settle_claim(&mut queue, &transaction_id, ClaimOutcome::NotYetClaimable, now);
        let pending = &queue["burn.json"];
        assert_eq!(pending.in_flight, None);
        assert_eq!(pending.failed_attempts, 1);
        assert!(pending.is_due(now));
    }

    #[test]
    fn an_accepted_or_failed_claim_leaves_the_queue() {
        let transaction_id = TransactionId::new([1; 32]);
        let mut queue = queue_with_claim_in_flight(transaction_id);
        settle_claim(&mut queue, &transaction_id, ClaimOutcome::Accepted, Instant::now());
        assert!(queue.is_empty());

        let mut queue = queue_with_claim_in_flight(transaction_id);
        settle_claim(
            &mut queue,
            &transaction_id,
            ClaimOutcome::Failed("invalid proof".to_string()),
            Instant::now(),
        );
        assert!(queue.is_empty());
    }

    #[test]
    fn another_transaction_does_not_settle_the_claim() {
        let transaction_id = TransactionId::new([1; 32]);
        let mut queue = queue_with_claim_in_flight(transaction_id);
        settle_claim(
            &mut queue,
            &TransactionId::new([2; 32]),
            ClaimOutcome::Accepted,
            Instant::now(),
        );
        assert_eq!(
            queue["burn.json"].in_flight.map(|c| c.transaction_id),
            Some(transaction_id)
        );
    }

    #[test]
    fn an_expired_claim_is_submitted_again_without_backing_off() {
        let transaction_id = TransactionId::new([1; 32]);
        let mut queue = queue_with_claim_in_flight(transaction_id);
        settle_claim(&mut queue, &transaction_id, ClaimOutcome::Expired, Instant::now());
        let pending = &queue["burn.json"];
        assert_eq!(pending.in_flight, None);
        assert_eq!(pending.failed_attempts, 0);
    }

    #[test]
    fn defers_burn_not_yet_synced_rejection() {
        assert!(is_burn_not_yet_claimable(&execution_failure(
            ExecutionFailureCode::NotYetValid
        )));
    }

    #[test]
    fn does_not_defer_other_rejections() {
        // A claim the verifier refused outright shares the instruction and the message shape with the
        // deferrable one, so only the code separates them.
        assert!(!is_burn_not_yet_claimable(&execution_failure(
            ExecutionFailureCode::InvalidProof
        )));
        assert!(!is_burn_not_yet_claimable(&execution_failure(
            ExecutionFailureCode::InsufficientFunds
        )));
        assert!(!is_burn_not_yet_claimable(&RejectReason::FailedToLockInputs(
            "input conflict".to_string()
        )));
    }
}
