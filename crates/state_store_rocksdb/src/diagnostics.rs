//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    cell::Cell,
    sync::{Mutex, MutexGuard, TryLockError},
    time::{Duration, Instant},
};

use rocksdb::TransactionDB;
use serde::{Serialize, de::DeserializeOwned};
use tari_ootle_common_types::{
    NodeAddressable,
    diagnostics::{DiagnosticEvent, DiagnosticEventFilter, DiagnosticEventRecord, unix_millis_now},
};
use tari_ootle_storage::{
    DiagnosticEventPage,
    DiagnosticEventStore,
    DiagnosticPruneStats,
    DiagnosticRetention,
    Ordering,
    StateStore,
    StorageError,
};

use crate::{column_families::diagnostic_event::DiagnosticEventCf, store::RocksDbStateStore};

static APPEND_LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    /// Whether this thread is between taking [`APPEND_LOCK`] and releasing it.
    static IS_APPENDING: Cell<bool> = const { Cell::new(false) };
}

/// How long [`acquire_append_lock`] waits for another thread before appending unserialised.
const APPEND_LOCK_TIMEOUT: Duration = Duration::from_millis(250);
const APPEND_LOCK_POLL: Duration = Duration::from_millis(5);

struct AppendGuard {
    _lock: Option<MutexGuard<'static, ()>>,
}

impl Drop for AppendGuard {
    fn drop(&mut self) {
        IS_APPENDING.with(|flag| flag.set(false));
    }
}

/// Serialises the read-then-write that assigns event ids.
///
/// Returns `None` when this thread already holds the lock. That happens when a thread panics inside
/// an append: the panic hook runs on the panicking thread before unwinding releases anything, so the
/// hook re-enters here while the outer write transaction is still open and holding rocksdb's own
/// locks on the keys it has written. Waiting cannot help — the holder is this thread — and the write
/// would only contend with that stalled transaction, so the caller skips the append instead.
///
/// Waiting on *another* thread is bounded so that a holder wedged mid-transaction cannot hang the
/// panic hook indefinitely; appending unserialised risks losing one racing event, which is the
/// better of the two outcomes.
fn acquire_append_lock() -> Option<AppendGuard> {
    if IS_APPENDING.with(Cell::get) {
        return None;
    }

    let deadline = Instant::now() + APPEND_LOCK_TIMEOUT;
    let lock = loop {
        match APPEND_LOCK.try_lock() {
            Ok(guard) => break Some(guard),
            Err(TryLockError::Poisoned(err)) => break Some(err.into_inner()),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(APPEND_LOCK_POLL);
            },
            Err(TryLockError::WouldBlock) => break None,
        }
    };

    IS_APPENDING.with(|flag| flag.set(true));
    Some(AppendGuard { _lock: lock })
}

impl<TAddr> DiagnosticEventStore for RocksDbStateStore<TAddr, TransactionDB>
where TAddr: NodeAddressable + Serialize + DeserializeOwned + 'static
{
    fn diagnostic_events_append(&self, events: &[DiagnosticEvent]) -> Result<Option<u64>, StorageError> {
        const OPERATION: &str = "diagnostic_events_append";
        if events.is_empty() {
            return Ok(None);
        }

        // The next id is derived from the highest key, and that read is not conflict-tracked, so two
        // appends racing would both pick the same id and one would overwrite the other. Appends come
        // from the writer task and from the panic hook, which can run on any thread at any moment,
        // so they are serialised here. Contention is effectively nil: both are rare, and the lock is
        // held only for the write transaction.
        let Some(_guard) = acquire_append_lock() else {
            return Err(StorageError::QueryError {
                reason: "diagnostic_events_append re-entered on a thread that is already appending".to_string(),
            });
        };

        self.with_write_tx(|tx| {
            let cf = tx.db().cf(DiagnosticEventCf)?;
            let mut next_id = cf
                .key_iterator(Ordering::Descending, OPERATION)
                .next()
                .transpose()?
                .map_or(0, |last| last.saturating_add(1));

            for event in events {
                cf.put(&next_id, event, OPERATION)?;
                next_id = next_id.saturating_add(1);
            }

            Ok(Some(next_id - 1))
        })
    }

    fn diagnostic_events_query(&self, page: &DiagnosticEventPage) -> Result<Vec<DiagnosticEventRecord>, StorageError> {
        const OPERATION: &str = "diagnostic_events_query";
        if page.limit == 0 {
            return Ok(Vec::new());
        }

        let tx = self.create_read_tx()?;
        let cf = tx.db().cf(DiagnosticEventCf)?;

        // Bounding the range on the cursor rather than skipping past it means a deep page seeks
        // straight to its first row instead of decoding every newer event on the way.
        let iter: Box<dyn Iterator<Item = _>> = match page.before_id {
            Some(before) => {
                let start = cf.encode_key(&0).to_vec();
                let end = cf.encode_key(&before).to_vec();
                Box::new(cf.range_iterator(Ordering::Descending, start..end))
            },
            None => Box::new(cf.iterator(Ordering::Descending, OPERATION)),
        };

        let mut records = Vec::new();
        for result in iter {
            let (id, event) = result?;
            if !page.filter.matches(&event) {
                continue;
            }
            records.push(DiagnosticEventRecord { id, event });
            if records.len() >= page.limit {
                break;
            }
        }

        Ok(records)
    }

    fn diagnostic_events_clear(&self, filter: &DiagnosticEventFilter) -> Result<usize, StorageError> {
        const OPERATION: &str = "diagnostic_events_clear";

        self.with_write_tx(|tx| {
            let cf = tx.db().cf(DiagnosticEventCf)?;
            let to_delete = cf
                .iterator(Ordering::Ascending, OPERATION)
                .filter_map(|result| match result {
                    Ok((id, event)) => filter.matches(&event).then_some(Ok(id)),
                    Err(err) => Some(Err(err)),
                })
                .collect::<Result<Vec<_>, _>>()?;

            for id in &to_delete {
                cf.delete(id, OPERATION)?;
            }

            Ok(to_delete.len())
        })
    }

    fn diagnostic_events_prune(&self, retention: DiagnosticRetention) -> Result<DiagnosticPruneStats, StorageError> {
        const OPERATION: &str = "diagnostic_events_prune";

        let now = unix_millis_now();
        let max_age_millis = u64::try_from(retention.max_age.as_millis()).unwrap_or(u64::MAX);

        self.with_write_tx(|tx| {
            let cf = tx.db().cf(DiagnosticEventCf)?;

            let Some(newest_id) = cf.key_iterator(Ordering::Descending, OPERATION).next().transpose()? else {
                return Ok(DiagnosticPruneStats::default());
            };

            // Ids only ever increase, so keeping ids above `newest - max_events` leaves at most
            // `max_events` rows: with no gaps exactly that many, with gaps fewer. `None` means the
            // log is shorter than the bound and nothing is over it.
            let count_cutoff = u64::try_from(retention.max_events)
                .ok()
                .and_then(|max| newest_id.checked_sub(max));

            let mut expired = Vec::new();
            let mut over_count = Vec::new();
            for result in cf.iterator(Ordering::Ascending, OPERATION) {
                let (id, event) = result?;
                if count_cutoff.is_some_and(|cutoff| id <= cutoff) {
                    over_count.push(id);
                    continue;
                }
                // Timestamps rise with the id, so the first event inside the age window ends the
                // walk. A clock stepping backwards can leave an older event behind until the count
                // bound evicts it.
                if now.saturating_sub(event.timestamp) <= max_age_millis {
                    break;
                }
                expired.push(id);
            }

            for id in over_count.iter().chain(expired.iter()) {
                cf.delete(id, OPERATION)?;
            }

            Ok(DiagnosticPruneStats {
                deleted_by_count: over_count.len(),
                deleted_by_age: expired.len(),
            })
        })
    }

    fn diagnostic_events_bounds(&self) -> Result<Option<(u64, u64)>, StorageError> {
        const OPERATION: &str = "diagnostic_events_bounds";

        let tx = self.create_read_tx()?;
        let cf = tx.db().cf(DiagnosticEventCf)?;
        let Some(oldest) = cf.key_iterator(Ordering::Ascending, OPERATION).next().transpose()? else {
            return Ok(None);
        };
        let newest = cf
            .key_iterator(Ordering::Descending, OPERATION)
            .next()
            .transpose()?
            .unwrap_or(oldest);

        Ok(Some((oldest, newest)))
    }
}
