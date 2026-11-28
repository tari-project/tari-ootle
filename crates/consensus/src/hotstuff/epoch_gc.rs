//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_storage::{
    EpochCleanupStep,
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};
use tokio::task;

use crate::{tracing::TraceTimer, traits::PeriodicTask};

const LOG_TARGET: &str = "tari::ootle::consensus::epoch_gc";

/// Records pruned per write transaction. The GC transactions run alongside consensus, so each is kept small enough that
/// its locks and write batch are held only briefly.
const EPOCH_GC_BATCH_SIZE: usize = 5_000;

pub struct EpochGc<TStore> {
    store: TStore,
}

impl<TStore: StateStore + Send + Sync + Clone + 'static> EpochGc<TStore> {
    pub fn new(store: TStore) -> Self {
        Self { store }
    }
}

impl<TStore: StateStore + Send + Sync + Clone + 'static> PeriodicTask for EpochGc<TStore> {
    fn name() -> &'static str {
        "🗑️ Epoch GC"
    }

    async fn do_work(&self) {
        let _timer = TraceTimer::info(LOG_TARGET, "🗑️ Epoch GC task")
            .with_excessive_threshold(std::time::Duration::from_secs(5));

        let store = self.store.clone();
        let result = task::spawn_blocking(move || run_epoch_cleanup(&store, EPOCH_GC_BATCH_SIZE)).await;

        match result {
            Ok(Ok(())) => {
                log::info!(target: LOG_TARGET, "🗑️ Epoch GC task completed successfully");
            },
            Ok(Err(err)) => {
                log::error!(target: LOG_TARGET, "Failed to run epoch GC: {}", err);
            },
            Err(e) => {
                log::error!(target: LOG_TARGET, "epoch GC panicked: {}", e);
            },
        }
    }
}

/// Runs every epoch cleanup step to completion, committing each batch of `batch_size` records in its own transaction.
fn run_epoch_cleanup<TStore: StateStore>(store: &TStore, batch_size: usize) -> Result<(), StorageError> {
    let epoch = store.with_read_tx(|tx| tx.current_epoch())?;
    for step in EpochCleanupStep::ALL {
        let mut total = 0usize;
        loop {
            let n = store.with_write_tx(|tx| tx.epoch_cleanup_step(epoch, step, batch_size))?;
            total += n;
            if n < batch_size {
                break;
            }
        }
        if total > 0 {
            log::info!(target: LOG_TARGET, "🗑️ Processed {total} {step} for epoch {epoch}");
        }
    }
    Ok(())
}
