//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::{
    models::WalletLockId,
    storage::{CommitableStore, WalletStoreWriter, WriteableWalletStore},
};

const LOG_TARGET: &str = "tari::ootle::wallet::models::lock_guard";

pub struct WalletLockDropGuard<'a, TStore: WriteableWalletStore> {
    lock_id: WalletLockId,
    store: Option<&'a TStore>,
}

impl<'a, TStore> WalletLockDropGuard<'a, TStore>
where TStore: WriteableWalletStore
{
    pub fn new(lock_id: WalletLockId, store: &'a TStore) -> Self {
        Self {
            lock_id,
            store: Some(store),
        }
    }

    pub fn id(&self) -> WalletLockId {
        self.lock_id
    }

    pub fn keep_locked(mut self) -> WalletLockId {
        self.store = None;
        self.lock_id
    }

    pub fn release(self) -> WalletLockId {
        self.lock_id
        // Drop will be called here, releasing the lock
    }
}

impl<TStore> Drop for WalletLockDropGuard<'_, TStore>
where TStore: WriteableWalletStore
{
    fn drop(&mut self) {
        let Some(store) = self.store.take() else {
            // Lock guard disarmed, do nothing
            return;
        };
        match store.create_write_tx() {
            Ok(mut tx) => {
                if let Err(e) = tx.locks_release(self.lock_id) {
                    log::error!(target: LOG_TARGET, "Failed to unlock wallet lock {:?}: {}", self.lock_id, e);
                }
                if let Err(e) = tx.commit() {
                    log::error!(target: LOG_TARGET, "Failed to commit unlock for wallet lock {:?}: {}", self.lock_id, e);
                }
            },
            Err(e) => {
                log::error!(
                    target: LOG_TARGET,
                    "Failed to create write transaction to unlock wallet lock {:?}: {}",
                    self.lock_id,
                    e
                );
            },
        };
    }
}
