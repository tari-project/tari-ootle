//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use log::*;
use tari_ootle_storage::{
    StorageError,
    time::{Duration as TimeDuration, OffsetDateTime, PrimitiveDateTime},
};
use tari_shutdown::ShutdownSignal;
use tokio::{task, time};

use crate::store::{IndexerStore, IndexerStoreWriteTransaction};

const LOG_TARGET: &str = "tari::indexer::transaction_pruner";

/// Transactions removed per write transaction. Bounds how long a single statement holds SQLite's
/// database-wide write lock, so a large backlog is cleared over several passes rather than one stall.
const BATCH_SIZE: usize = 500;

/// `tokio::time::interval` panics on a zero period, which would kill the pruner task, so a
/// configured interval is raised to this floor.
const MIN_INTERVAL: Duration = Duration::from_secs(1);

/// Periodically deletes transactions submitted through this indexer once they age past the retention
/// window. Transaction receipts synced from the network are keyed independently and are untouched.
pub struct TransactionPruner<TStore> {
    store: TStore,
    retention: Duration,
    interval: Duration,
}

impl<TStore: IndexerStore + Clone> TransactionPruner<TStore> {
    pub fn new(store: TStore, retention: Duration, interval: Duration) -> Self {
        Self {
            store,
            retention,
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
                        info!(target: LOG_TARGET, "🧹 Transaction pruner was shutdown.");
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
                                            "🧹 Pruned {num_pruned} transaction(s) older than {:.0?}",
                                            self.retention,
                                        );
                                    } else {
                                        debug!(target: LOG_TARGET, "🧹 No transactions to prune.");
                                    }
                                    num_pruned = 0;
                                }
                            },
                            Err(err) => {
                                error!(target: LOG_TARGET, "⚠️ Transaction pruning failed: {err}");
                                num_pruned = 0;
                            },
                        }
                    },
                }
            }
        })
    }

    async fn prune_batch(&self) -> Result<usize, StorageError> {
        let cutoff = cutoff_from(OffsetDateTime::now_utc(), self.retention);
        self.store
            .with_write_tx(move |tx| tx.prune_transactions_before(cutoff, BATCH_SIZE))
            .await
    }
}

/// `created_at` is written by SQLite's `current_timestamp`, which is UTC, so the cutoff is the UTC
/// wall clock less the retention window. A retention window so large that it underflows yields the
/// UNIX epoch, which predates every row and therefore prunes nothing.
fn cutoff_from(now: OffsetDateTime, retention: Duration) -> PrimitiveDateTime {
    let retention = TimeDuration::try_from(retention).unwrap_or(TimeDuration::MAX);
    let cutoff = now.checked_sub(retention).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    PrimitiveDateTime::new(cutoff.date(), cutoff.time())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_unix_secs(secs: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(secs).unwrap()
    }

    #[test]
    fn cutoff_subtracts_the_retention_window() {
        let now = at_unix_secs(1_800_000_000);
        let cutoff = cutoff_from(now, Duration::from_secs(2 * 60 * 60));
        assert_eq!(cutoff.assume_utc(), at_unix_secs(1_800_000_000 - 2 * 60 * 60));
    }

    #[test]
    fn cutoff_saturates_instead_of_overflowing() {
        let cutoff = cutoff_from(at_unix_secs(1_800_000_000), Duration::MAX);
        assert_eq!(cutoff.assume_utc(), OffsetDateTime::UNIX_EPOCH);
    }
}
