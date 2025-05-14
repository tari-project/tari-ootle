//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use log::{error, info};
use tari_dan_common_types::NumPreshards;
use tari_dan_storage::{StateStore, StateStoreWriteTransaction};
use tokio::{task, task::AbortHandle};

use crate::tracing::TraceTimer;

const LOG_TARGET: &str = "tari::dan::consensus::state_tree_gc";

pub struct StateTreeGc<TStore> {
    store: TStore,
    num_preshards: NumPreshards,
}

impl<TStore: StateStore + Send + Sync + Clone + 'static> StateTreeGc<TStore> {
    pub fn new(store: TStore, num_preshards: NumPreshards) -> Self {
        Self { store, num_preshards }
    }

    pub fn do_work_periodically(self, interval: Duration) -> AbortOnDropGuard {
        let handle = tokio::spawn(async move {
            info!(target: LOG_TARGET, "🗑️ State tree GC task starting periodically every {:.2?}", interval);
            loop {
                tokio::time::sleep(interval).await;
                let store = self.store.clone();

                // NOTE: this task will not be aborted until after completion. When AbortOnDropGuard is dropped, these
                // could happen:
                // - this task is awaiting sleep - it will be aborted immediately
                // - this task is awaiting the spawn_blocking task - the task will be aborted immediately but the
                //   spawn_blocking task will continue until completed. This could delay node shutdown.
                let result = task::spawn_blocking(move || {
                    let _timer = TraceTimer::info(LOG_TARGET, "🗑️ StatTreeGc task")
                        .with_excessive_threshold(Duration::from_secs(5));
                    info!(target: LOG_TARGET, "🗑️ State tree GC task starting");
                    // NOTE: this is the only task that writes to the state store concurrently. This is safe
                    // because we are clearing keys that are no longer part of the state tree.
                    // Rocks' TransactionDb locks on the key level.
                    store.with_write_tx(|tx| tx.state_tree_nodes_clear_stale(self.num_preshards))
                })
                .await;

                match result {
                    Ok(Ok(_)) => {
                        info!(target: LOG_TARGET, "🗑️ State tree GC task completed successfully");
                    },
                    Ok(Err(err)) => {
                        error!(target: LOG_TARGET, "Failed to run state tree GC: {}", err);
                    },
                    Err(e) => {
                        // This should only be from a panic
                        error!(target: LOG_TARGET, "Failed to run state tree GC: {}", e);
                    },
                }
            }
        });

        AbortOnDropGuard::new(handle.abort_handle())
    }
}

pub struct AbortOnDropGuard {
    handle: AbortHandle,
}

impl AbortOnDropGuard {
    fn new(handle: AbortHandle) -> Self {
        Self { handle }
    }
}

impl Drop for AbortOnDropGuard {
    fn drop(&mut self) {
        self.handle.abort();
        info!(target: LOG_TARGET, "🗑️ State tree GC task aborted");
    }
}
