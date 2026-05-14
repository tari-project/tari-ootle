//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

pub trait SyncManager {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Determine whether state sync is required. Takes `&mut self` so the implementation may
    /// record information for the subsequent [`SyncManager::sync`] call — for example, the
    /// target epoch resolved by a stall-recovery probe.
    fn check_sync(&mut self) -> impl Future<Output = Result<SyncStatus, Self::Error>> + Send;

    fn sync(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    UpToDate,
    Behind,
    /// The sync manager could not determine whether we are behind or up to date — for example,
    /// not enough committee members responded to a recovery probe. The state machine should
    /// back off and retry [`SyncManager::check_sync`] without entering the syncing phase.
    Inconclusive,
}
