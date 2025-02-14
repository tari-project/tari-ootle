//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus::{
    hotstuff::HotStuffError,
    traits::{SyncManager, SyncStatus},
};

#[derive(Clone)]
pub struct AlwaysSyncedSyncManager;

impl SyncManager for AlwaysSyncedSyncManager {
    type Error = HotStuffError;

    async fn check_sync(&self) -> Result<SyncStatus, Self::Error> {
        Ok(SyncStatus::UpToDate)
    }

    async fn sync(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
