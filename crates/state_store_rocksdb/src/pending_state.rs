//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The in-memory state of uncommitted blocks: the substate locks they hold ([`LockTable`]), the substate changes
//! they make ([`BlockDiffTable`]) and their state tree diffs ([`TreeDiffTable`]).
//!
//! Each table answers every read of its kind. Its persistent form is one record per block, written in the same
//! RocksDB transaction as the change that produced it, and the tables are rebuilt from those records when the store
//! opens.
//!
//! Each write transaction stages its changes on a private copy of the state and publishes them only once its RocksDB
//! commit has succeeded, so the state never holds anything the database does not.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::{
    block_diff_table::BlockDiffTable,
    error::RocksDbStorageError,
    lock_table::LockTable,
    tree_diff_table::TreeDiffTable,
};

/// Cloning shares every table; a staged copy copies a table only when it first changes it.
#[derive(Debug, Clone, Default)]
pub(crate) struct PendingState {
    pub locks: Arc<LockTable>,
    pub block_diffs: Arc<BlockDiffTable>,
    pub tree_diffs: Arc<TreeDiffTable>,
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

    pub fn tree_diffs_mut(&mut self) -> &mut TreeDiffTable {
        Arc::make_mut(&mut self.state.tree_diffs)
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
        read: OnceLock<StagedPendingState>,
        staged: Option<StagedPendingState>,
    },
}

impl<'a> PendingStateView<'a> {
    pub fn writer(shared: &'a SharedPendingState) -> Self {
        Self::Writer {
            shared,
            read: OnceLock::new(),
            staged: None,
        }
    }

    pub fn state(&self) -> PendingState {
        match self {
            Self::Pinned(state) => state.clone(),
            Self::Writer {
                staged: Some(staged), ..
            } => staged.state().clone(),
            Self::Writer {
                shared,
                read,
                staged: None,
            } => read.get_or_init(|| shared.stage()).state().clone(),
        }
    }

    /// The staged copy changes are applied to, created from the latest published state on first use.
    ///
    /// # Panics
    /// On a pinned view, which belongs to a read view and is never written.
    pub fn staged_mut(&mut self) -> &mut StagedPendingState {
        match self {
            Self::Pinned(_) => panic!("a read view's pending state is never written"),
            Self::Writer { shared, read, staged } => {
                staged.get_or_insert_with(|| read.take().unwrap_or_else(|| shared.stage()))
            },
        }
    }

    /// The state to publish to and the changes to publish, if this transaction changed any.
    pub fn into_staged(self) -> Option<(&'a SharedPendingState, StagedPendingState)> {
        match self {
            Self::Pinned(_) => None,
            Self::Writer { shared, staged, .. } => staged.map(|staged| (shared, staged)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_rejects_commit_when_published_state_changes_between_read_and_stage() {
        let shared = SharedPendingState::default();

        let mut writer1 = PendingStateView::writer(&shared);
        let _initial_read = writer1.state();

        let mut writer2 = PendingStateView::writer(&shared);
        let _ = writer2.staged_mut();
        let (shared2, staged2) = writer2.into_staged().unwrap();
        shared2.commit_and_publish(staged2, || Ok(())).unwrap();

        let _ = writer1.staged_mut();
        let (shared1, staged1) = writer1.into_staged().unwrap();
        let mut committed = false;
        let err = shared1
            .commit_and_publish(staged1, || {
                committed = true;
                Ok(())
            })
            .unwrap_err();
        assert!(!committed);
        assert!(matches!(err, RocksDbStorageError::GeneralError { .. }));
    }

    #[test]
    fn read_only_writer_does_not_produce_staged_state() {
        let shared = SharedPendingState::default();
        let writer = PendingStateView::writer(&shared);
        let _ = writer.state();
        assert!(writer.into_staged().is_none());
    }
}
