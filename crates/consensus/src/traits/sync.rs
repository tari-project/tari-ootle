//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

use tari_ootle_common_types::{Epoch, ShardGroup};

pub trait SyncManager {
    type Error: std::error::Error + Send + Sync + 'static;

    fn check_sync(&self) -> impl Future<Output = Result<SyncStatus, Self::Error>> + Send;

    /// Synchronise state. `target_epoch` is the epoch the caller has resolved to be the highest
    /// finalised epoch the network has reached (e.g. via a stall-recovery probe). `None` means
    /// "use your usual fallback" — typically the oracle's current epoch.
    fn sync(&mut self, target_epoch: Option<Epoch>) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Synchronise `shard_group`'s state, for a node that joins its committee after `epoch`, to the checkpoint that
    /// ends the epoch before `epoch`. Repeating it in a later epoch extends the state already synced.
    fn presync(
        &mut self,
        shard_group: ShardGroup,
        epoch: Epoch,
    ) -> impl Future<Output = Result<PresyncStatus, Self::Error>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresyncStatus {
    Synced,
    /// No peer holds the checkpoint ending the epoch before the presync's epoch yet. The committee stores it once it
    /// commits that epoch's end, shortly after the epoch changes, so a later attempt in the same epoch succeeds.
    CheckpointNotAvailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    UpToDate,
    /// State sync is required. `target_epoch` carries the epoch the caller proved to be the
    /// highest finalised one (e.g. via a stall-recovery probe). `None` means the caller has no
    /// specific target and the syncing implementation should fall back to its default — usually
    /// the oracle's current epoch.
    Behind {
        target_epoch: Option<Epoch>,
    },
    /// The sync manager could not determine whether we are behind or up to date — for example,
    /// not enough committee members responded to a recovery probe. The state machine should
    /// back off and retry [`SyncManager::check_sync`] without entering the syncing phase.
    Inconclusive,
}
