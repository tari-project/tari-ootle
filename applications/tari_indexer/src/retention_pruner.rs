//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use log::*;
use tari_epoch_manager::service::EpochManagerHandle;
use tari_ootle_common_types::Epoch;
use tari_ootle_p2p::PeerAddress;
use tari_ootle_storage::StorageError;
use tari_shutdown::ShutdownSignal;
use tokio::{task, time};

use crate::store::{IndexerStore, IndexerStoreWriteTransaction};

const LOG_TARGET: &str = "tari::indexer::retention_pruner";

/// Rows removed per write transaction. Bounds how long a single statement holds SQLite's
/// database-wide write lock, so a large backlog is cleared over several passes rather than one stall.
const BATCH_SIZE: usize = 500;

/// `tokio::time::interval` panics on a zero period, which would kill the pruner task, so a
/// configured interval is raised to this floor.
const MIN_INTERVAL: Duration = Duration::from_secs(1);

/// The rows a [`RetentionPruner`] deletes, each aged by its own epoch column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneTarget {
    /// Stored transaction bodies, aged by their retention epoch.
    Transactions,
    /// Transaction receipts, aged by the epoch the transaction committed in.
    TransactionReceipts,
    /// Events, aged by the epoch the transaction that emitted them committed in.
    Events,
}

impl PruneTarget {
    fn noun(self) -> &'static str {
        match self {
            Self::Transactions => "transaction(s)",
            Self::TransactionReceipts => "transaction receipt(s)",
            Self::Events => "event(s)",
        }
    }
}

/// Periodically deletes one [`PruneTarget`]'s rows once their epoch falls more than
/// `retention_epochs` behind the current epoch.
pub struct RetentionPruner<TStore> {
    store: TStore,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    target: PruneTarget,
    retention_epochs: u64,
    interval: Duration,
}

impl<TStore: IndexerStore + Clone> RetentionPruner<TStore> {
    pub fn new(
        store: TStore,
        epoch_manager: EpochManagerHandle<PeerAddress>,
        target: PruneTarget,
        retention_epochs: u64,
        interval: Duration,
    ) -> Self {
        Self {
            store,
            epoch_manager,
            target,
            retention_epochs,
            interval: interval.max(MIN_INTERVAL),
        }
    }

    pub fn spawn(self, mut shutdown: ShutdownSignal) -> task::JoinHandle<()> {
        task::spawn(async move {
            let mut interval = time::interval(self.interval);
            interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
            let mut num_pruned = 0;

            loop {
                tokio::select! {
                    _ = shutdown.wait() => {
                        info!(target: LOG_TARGET, "🧹 Retention pruner for {:?} was shutdown.", self.target);
                        break;
                    },
                    // The first tick resolves immediately, clearing whatever aged out while the
                    // indexer was down.
                    _ = interval.tick() => {
                        match self.prune_batch().await {
                            Ok(deleted) => {
                                num_pruned += deleted;
                                if deleted == BATCH_SIZE {
                                    // A full batch means more rows are already past the cutoff. Waiting a
                                    // whole interval per batch would cap the drain rate at a fixed number
                                    // of rows per interval, under which a busy indexer still grows without
                                    // bound. Returning through `select!` between batches keeps shutdown
                                    // responsive and lets other writers take the lock.
                                    interval.reset_immediately();
                                } else {
                                    if num_pruned > 0 {
                                        info!(
                                            target: LOG_TARGET,
                                            "🧹 Pruned {num_pruned} {} more than {} epoch(s) behind",
                                            self.target.noun(),
                                            self.retention_epochs,
                                        );
                                    } else {
                                        debug!(target: LOG_TARGET, "🧹 No {} to prune.", self.target.noun());
                                    }
                                    num_pruned = 0;
                                }
                            },
                            Err(err) => {
                                error!(target: LOG_TARGET, "⚠️ Pruning {} failed: {err}", self.target.noun());
                                num_pruned = 0;
                            },
                        }
                    },
                }
            }
        })
    }

    async fn prune_batch(&self) -> Result<usize, StorageError> {
        let current_epoch = self.epoch_manager.get_current_epoch();
        // The epoch manager reports zero until its initial scan completes. Pruning against it would
        // measure every row against an epoch the network has long passed.
        if current_epoch.is_zero() {
            return Ok(0);
        }

        let cutoff = cutoff_from(current_epoch, self.retention_epochs);
        let target = self.target;
        self.store
            .with_write_tx(move |tx| match target {
                PruneTarget::Transactions => tx.prune_transactions_before_epoch(cutoff, BATCH_SIZE),
                PruneTarget::TransactionReceipts => tx.prune_transaction_receipts_before_epoch(cutoff, BATCH_SIZE),
                PruneTarget::Events => tx.prune_events_before_epoch(cutoff, BATCH_SIZE),
            })
            .await
    }
}

/// A row is retained while its epoch is within `retention_epochs` of the current epoch, so the
/// cutoff — the first epoch still retained — is `current - retention_epochs`. With a window of zero
/// that is the current epoch itself, which for transactions prunes everything that can no longer
/// commit.
fn cutoff_from(current_epoch: Epoch, retention_epochs: u64) -> Epoch {
    current_epoch.saturating_sub(Epoch(retention_epochs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutoff_is_the_oldest_retained_epoch() {
        assert_eq!(cutoff_from(Epoch(100), 10), Epoch(90));
    }

    #[test]
    fn a_zero_window_retains_only_the_current_epoch() {
        assert_eq!(cutoff_from(Epoch(100), 0), Epoch(100));
    }

    #[test]
    fn cutoff_saturates_instead_of_underflowing() {
        assert_eq!(cutoff_from(Epoch(3), 10), Epoch::zero());
    }
}
