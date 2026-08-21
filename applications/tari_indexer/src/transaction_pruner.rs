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

/// Passes per run. Caps the work a single tick can do; whatever remains is picked up on the next tick.
const MAX_BATCHES_PER_RUN: usize = 20;

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
            interval,
        }
    }

    pub fn spawn(self, mut shutdown: ShutdownSignal) -> task::JoinHandle<()> {
        task::spawn(async move {
            let mut interval = time::interval(self.interval);
            interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = shutdown.wait() => {
                        info!(target: LOG_TARGET, "🧹 Transaction pruner was shutdown.");
                        break;
                    },
                    // The first tick resolves immediately, clearing whatever aged out while the
                    // indexer was down.
                    _ = interval.tick() => {
                        match self.prune().await {
                            Ok(0) => {
                                debug!(target: LOG_TARGET, "🧹 No transactions to prune.");
                            },
                            Ok(num_pruned) => {
                                info!(target: LOG_TARGET, "🧹 Pruned {num_pruned} transaction(s) older than {:.0?}", self.retention);
                            },
                            Err(err) => {
                                error!(target: LOG_TARGET, "⚠️ Transaction pruning failed: {err}");
                            },
                        }
                    },
                }
            }
        })
    }

    async fn prune(&self) -> Result<usize, StorageError> {
        let cutoff = cutoff_from(OffsetDateTime::now_utc(), self.retention);
        let mut num_pruned = 0;
        for _ in 0..MAX_BATCHES_PER_RUN {
            let deleted = self
                .store
                .with_write_tx(move |tx| tx.prune_transactions_before(cutoff, BATCH_SIZE))
                .await?;
            num_pruned += deleted;
            if deleted < BATCH_SIZE {
                break;
            }
        }
        Ok(num_pruned)
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
