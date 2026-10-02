//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use log::*;
use prometheus_client::{
    metrics::{
        counter::Counter,
        gauge::Gauge,
        histogram::{Histogram, exponential_buckets},
    },
    registry::Registry,
};
use tari_shutdown::ShutdownSignal;
use tokio::{task, time};

use crate::{metrics::CollectorRegister, storage_sqlite::SqliteIndexerStore};

const LOG_TARGET: &str = "tari::indexer::storage_sqlite::metrics";

/// How often the database and WAL file gauges are refreshed.
const FILE_STATS_INTERVAL: Duration = Duration::from_secs(30);

/// Contention and growth of the indexer's SQLite database.
///
/// SQLite admits one write transaction at a time, so every writer - state sync, the substate cache
/// and the pruners - queues on the same lock. `write_lock_wait_seconds` is that queue and
/// `write_lock_hold_seconds` is what each writer makes the others wait; a writer that waits out
/// the busy timeout fails and counts in `write_lock_busy_total`.
///
/// The WAL gauges show whether readers are holding back checkpoints. A checkpoint can only copy
/// frames back into the database up to the oldest open read transaction, so under constant reads
/// `wal_uncheckpointed_frames` stays high and the WAL file keeps growing, slowing reads and writes
/// alike.
#[derive(Debug, Clone)]
pub struct StorageMetrics {
    connection_wait: Histogram,
    write_lock_wait: Histogram,
    write_lock_hold: Histogram,
    write_lock_busy: Counter,
    db_bytes: Gauge,
    wal_bytes: Gauge,
    wal_uncheckpointed_frames: Gauge,
    freelist_bytes: Gauge,
}

impl StorageMetrics {
    pub fn register(registry: &mut Registry) -> Self {
        let registry = registry.sub_registry_with_prefix("sqlite");
        Self {
            connection_wait: Histogram::new(exponential_buckets(0.0001, 2.0, 17)).register_at(
                "connection_wait_seconds",
                "Time spent waiting for a pooled database connection, for reads and writes alike",
                registry,
            ),
            write_lock_wait: Histogram::new(exponential_buckets(0.0001, 2.0, 17)).register_at(
                "write_lock_wait_seconds",
                "Time a write transaction spent waiting for the database write lock",
                registry,
            ),
            write_lock_hold: Histogram::new(exponential_buckets(0.0001, 2.0, 17)).register_at(
                "write_lock_hold_seconds",
                "Time a write transaction held the database write lock, from acquiring it to commit or rollback",
                registry,
            ),
            write_lock_busy: Counter::default().register_at(
                "write_lock_busy",
                "Number of write transactions that gave up waiting for the database write lock",
                registry,
            ),
            db_bytes: Gauge::default().register_at("db_bytes", "Size of the database file in bytes", registry),
            wal_bytes: Gauge::default().register_at(
                "wal_bytes",
                "Size of the write-ahead log file in bytes. SQLite reuses the file rather than shrinking it, so this \
                 is the largest the log has grown since it was last reset",
                registry,
            ),
            wal_uncheckpointed_frames: Gauge::default().register_at(
                "wal_uncheckpointed_frames",
                "Write-ahead log frames a passive checkpoint could not copy back into the database, because an open \
                 read transaction still needs them",
                registry,
            ),
            freelist_bytes: Gauge::default().register_at(
                "freelist_bytes",
                "Bytes of free pages in the database file, reused for new rows before the file grows and returned to \
                 the filesystem only by VACUUM",
                registry,
            ),
        }
    }

    pub(super) fn observe_connection_wait(&self, wait: Duration) {
        self.connection_wait.observe(wait.as_secs_f64());
    }

    pub(super) fn observe_write_lock_wait(&self, wait: Duration) {
        self.write_lock_wait.observe(wait.as_secs_f64());
    }

    pub(super) fn observe_write_lock_hold(&self, hold: Duration) {
        self.write_lock_hold.observe(hold.as_secs_f64());
    }

    pub(super) fn inc_write_lock_busy(&self) {
        self.write_lock_busy.inc();
    }

    fn set_file_stats(&self, stats: &StorageFileStats) {
        self.db_bytes.set(saturating_i64(stats.db_bytes));
        self.wal_bytes.set(saturating_i64(stats.wal_bytes));
        self.wal_uncheckpointed_frames
            .set(saturating_i64(stats.wal_uncheckpointed_frames));
        self.freelist_bytes.set(saturating_i64(stats.freelist_bytes));
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct StorageFileStats {
    pub db_bytes: u64,
    pub wal_bytes: u64,
    pub wal_uncheckpointed_frames: u64,
    pub freelist_bytes: u64,
}

/// Refreshes the file gauges of `metrics` from `store` every [`FILE_STATS_INTERVAL`] until shutdown.
pub fn spawn_file_stats_sampler(
    store: SqliteIndexerStore,
    metrics: StorageMetrics,
    mut shutdown: ShutdownSignal,
) -> task::JoinHandle<()> {
    task::spawn(async move {
        let mut interval = time::interval(FILE_STATS_INTERVAL);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = shutdown.wait() => break,
                _ = interval.tick() => match store.file_stats().await {
                    Ok(stats) => metrics.set_file_stats(&stats),
                    Err(err) => warn!(target: LOG_TARGET, "⚠️ Failed to read database file stats: {err}"),
                },
            }
        }
    })
}

fn saturating_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}
