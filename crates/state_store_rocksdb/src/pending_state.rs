//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The in-memory state of uncommitted blocks: the substate locks they hold ([`LockTable`]) and the substate changes
//! they make ([`BlockDiffTable`]).
//!
//! Each table answers every read of its kind. Its persistent form is one record per block, written in the same
//! RocksDB transaction as the change that produced it, and the tables are rebuilt from those records when the store
//! opens.
//!
//! Each write transaction stages its changes on a private copy of the state and publishes them only once its RocksDB
//! commit has succeeded, so the state never holds anything the database does not.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::{block_diff_table::BlockDiffTable, error::RocksDbStorageError, lock_table::LockTable};

/// Cloning shares both tables; a staged copy copies a table only when it first changes it.
#[derive(Debug, Clone, Default)]
pub(crate) struct PendingState {
    pub locks: Arc<LockTable>,
    pub block_diffs: Arc<BlockDiffTable>,
}

/// The pending state every transaction of one store reads and publishes to.
#[derive(Debug, Default)]
pub(crate) struct SharedPendingState {
    published: Mutex<Published>,
}

#[derive(Debug, Default)]
struct Published {
    /// Incremented by every publish, so a staged copy can tell whether it is still based on the latest state.
    generation: u64,
    state: PendingState,
}

impl SharedPendingState {
    pub fn new(state: PendingState) -> Self {
        Self {
            published: Mutex::new(Published { generation: 0, state }),
        }
    }

    fn published(&self) -> MutexGuard<'_, Published> {
        // The guarded state is replaced only after the RocksDB commit it mirrors has succeeded, so a panic while
        // holding the lock cannot leave it describing an uncommitted change.
        self.published.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The latest published state.
    pub fn latest(&self) -> PendingState {
        self.with_published(PendingState::clone)
    }

    /// Runs `f` on the published state, holding off publishing until it returns. Publishing waits for the commit it
    /// mirrors, so a database snapshot taken inside `f` describes the same committed state as the state `f` is given.
    pub fn with_published<R, F: FnOnce(&PendingState) -> R>(&self, f: F) -> R {
        f(&self.published().state)
    }

    fn stage(&self) -> StagedPendingState {
        let published = self.published();
        StagedPendingState {
            base_generation: published.generation,
            state: published.state.clone(),
        }
    }

    /// Commits the database transaction with `commit` and, if it succeeds, publishes `staged`.
    ///
    /// A staged copy describes the state it was copied from plus this transaction's changes, and the records the
    /// transaction wrote were computed from it. If another transaction has published since, both are out of date, so
    /// the database transaction is left uncommitted and an error is returned.
    pub fn commit_and_publish<F: FnOnce() -> Result<(), RocksDbStorageError>>(
        &self,
        staged: StagedPendingState,
        commit: F,
    ) -> Result<(), RocksDbStorageError> {
        let mut published = self.published();
        if published.generation != staged.base_generation {
            return Err(RocksDbStorageError::GeneralError {
                message: "Write transaction not committed: another write transaction changed the pending block state \
                          after this one read it"
                    .to_string(),
            });
        }
        commit()?;
        published.state = staged.state;
        published.generation += 1;
        Ok(())
    }
}

/// A write transaction's changes, applied to a private copy of the state it read from.
#[derive(Debug)]
pub(crate) struct StagedPendingState {
    base_generation: u64,
    state: PendingState,
}

impl StagedPendingState {
    pub fn state(&self) -> &PendingState {
        &self.state
    }

    pub fn locks_mut(&mut self) -> &mut LockTable {
        Arc::make_mut(&mut self.state.locks)
    }

    pub fn block_diffs_mut(&mut self) -> &mut BlockDiffTable {
        Arc::make_mut(&mut self.state.block_diffs)
    }
}

/// The pending state a transaction reads from.
pub(crate) enum PendingStateView<'a> {
    /// The state published when a read view's snapshot was taken.
    Pinned(PendingState),
    /// A write transaction's state: the latest published one, until the transaction changes it, and from then on its
    /// staged copy.
    Writer {
        shared: &'a SharedPendingState,
        staged: Option<StagedPendingState>,
    },
}

impl<'a> PendingStateView<'a> {
    pub fn writer(shared: &'a SharedPendingState) -> Self {
        Self::Writer { shared, staged: None }
    }

    pub fn state(&self) -> PendingState {
        match self {
            Self::Pinned(state) => state.clone(),
            Self::Writer {
                staged: Some(staged), ..
            } => staged.state().clone(),
            Self::Writer { shared, staged: None } => shared.latest(),
        }
    }

    /// The staged copy changes are applied to, created from the latest published state on first use.
    ///
    /// # Panics
    /// On a pinned view, which belongs to a read view and is never written.
    pub fn staged_mut(&mut self) -> &mut StagedPendingState {
        match self {
            Self::Pinned(_) => panic!("a read view's pending state is never written"),
            Self::Writer { shared, staged } => staged.get_or_insert_with(|| shared.stage()),
        }
    }

    /// The state to publish to and the changes to publish, if this transaction changed any.
    pub fn into_staged(self) -> Option<(&'a SharedPendingState, StagedPendingState)> {
        match self {
            Self::Pinned(_) => None,
            Self::Writer { shared, staged } => staged.map(|staged| (shared, staged)),
        }
    }
}
