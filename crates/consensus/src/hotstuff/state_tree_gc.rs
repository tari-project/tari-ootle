//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::{error, info};
use tari_ootle_common_types::NumPreshards;
use tari_ootle_storage::{StateStore, StateStoreWriteTransaction, StateTreeRetention, StorageError};
use tokio::task;

use crate::traits::PeriodicTask;

const LOG_TARGET: &str = "tari::ootle::consensus::state_tree_gc";

/// Rows deleted per write transaction. The GC transactions run alongside consensus, so each is kept small enough that
/// its locks and write batch are held only briefly.
const STATE_TREE_GC_MAX_DELETES: usize = 5_000;

pub struct StateTreeGc<TStore> {
    store: TStore,
    num_preshards: NumPreshards,
    pinned_checkpoint_epochs: u64,
}

impl<TStore: StateStore + Send + Sync + Clone + 'static> StateTreeGc<TStore> {
    /// A GC that keeps the whole tree of each shard at its version in each of the last `pinned_checkpoint_epochs`
    /// epoch checkpoints stored, so that peers can read a shard's tree at a recent checkpoint.
    pub fn new(store: TStore, num_preshards: NumPreshards, pinned_checkpoint_epochs: u64) -> Self {
        Self {
            store,
            num_preshards,
            pinned_checkpoint_epochs,
        }
    }
}

impl<TStore: StateStore + Send + Sync + Clone + 'static> PeriodicTask for StateTreeGc<TStore> {
    fn name() -> &'static str {
        "🗑️ StateTreeGc"
    }

    async fn do_work(&self) {
        let store = self.store.clone();
        let num_preshards = self.num_preshards;
        let pinned_checkpoint_epochs = self.pinned_checkpoint_epochs;

        // NOTE: this task will not be aborted until after completion. When AbortOnDropGuard is dropped, these
        // could happen:
        // - this task is awaiting sleep - it will be aborted immediately
        // - this task is awaiting the spawn_blocking task - the task will be aborted immediately but the spawn_blocking
        //   task will continue until completed. This could delay node shutdown.
        let result = task::spawn_blocking(move || {
            // NOTE: this task writes to the state store concurrently. This is safe
            // because we are clearing keys that are no longer part of the state tree.
            // Rocks' TransactionDb locks on the key level.
            run_state_tree_gc(
                &store,
                num_preshards,
                pinned_checkpoint_epochs,
                STATE_TREE_GC_MAX_DELETES,
            )
        })
        .await;

        match result {
            Ok(Ok(n)) => {
                info!(target: LOG_TARGET, "🗑️ State tree GC task completed successfully. Wrote {n} rows deleting or keeping stale nodes");
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
}

/// Deletes every stale state tree node past the history window that no pinned tree holds, committing at most about
/// `max_writes` rows per transaction. Returns the number of rows written.
fn run_state_tree_gc<TStore: StateStore>(
    store: &TStore,
    num_preshards: NumPreshards,
    pinned_checkpoint_epochs: u64,
    max_writes: usize,
) -> Result<usize, StorageError> {
    let mut total = 0usize;
    loop {
        let n = store.with_write_tx(|tx| {
            let retention = StateTreeRetention::read(&**tx, num_preshards, pinned_checkpoint_epochs)?;
            tx.state_tree_nodes_clear_stale(&retention, max_writes)
        })?;
        total += n;
        if n < max_writes {
            return Ok(total);
        }
    }
}
