//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::{StateStore, StateStoreReadTransaction, StateStoreWriteTransaction};
use tokio::task;

use crate::{tracing::TraceTimer, traits::PeriodicTask};

const LOG_TARGET: &str = "tari::dan::consensus::state_tree_gc";

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
        let result = task::spawn_blocking(move || {
            store.with_write_tx(|tx| {
                let db_epoch = tx.current_epoch()?;
                tx.epoch_cleanup(db_epoch)
            })
        })
        .await;

        match result {
            Ok(_) => {
                log::info!(target: LOG_TARGET, "🗑️ Epoch GC task completed successfully");
            },
            Err(e) => {
                log::error!(target: LOG_TARGET, "Failed to run epoch GC: {}", e);
            },
        }
    }
}
