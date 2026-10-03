//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::{Seq, serde_as};
use tari_ootle_common_types::{Epoch, ShardGroup, StateVersion, shard::Shard};
use tokio::sync::{Mutex, MutexGuard};

#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncProgress {
    pub last_epoch: Epoch,
    // These transform the maps into Vecs for JSON serialization (can't have non-string keys).
    #[serde_as(as = "Seq<(_, _)>")]
    pub checkpoint_progress: IndexMap<ShardGroup, Epoch>,
    #[serde_as(as = "Seq<(_, _)>")]
    pub last_state_versions: IndexMap<Shard, (StateVersion, Epoch)>,
}

impl SyncProgress {
    pub fn checkpoint_epoch(&self, shard_group: ShardGroup) -> Option<Epoch> {
        self.checkpoint_progress.get(&shard_group).copied()
    }

    pub fn record_checkpoint(&mut self, shard_group: ShardGroup, epoch: Epoch) {
        self.checkpoint_progress.insert(shard_group, epoch);
    }

    pub fn last_state_version(&self, shard: Shard) -> Option<StateVersion> {
        self.last_state_versions.get(&shard).map(|(version, _)| *version)
    }

    pub fn record_state_version(&mut self, shard: Shard, state_version: StateVersion, epoch: Epoch) {
        self.last_state_versions.insert(shard, (state_version, epoch));
    }

    pub fn forget_state_version(&mut self, shard: Shard) {
        self.last_state_versions.shift_remove(&shard);
    }

    /// The shards whose recorded state version is above `max_version` or was recorded at an epoch
    /// after `max_epoch`.
    pub fn shards_recorded_beyond(&self, max_version: StateVersion, max_epoch: Epoch) -> Vec<Shard> {
        self.last_state_versions
            .iter()
            .filter(|(_, (version, epoch))| *version > max_version || *epoch > max_epoch)
            .map(|(shard, _)| *shard)
            .collect()
    }
}

/// The sync progress shared by every shard-group stream, each of which advances its own shards.
///
/// Progress is persisted as a single value, so a stream holds the lock from taking the snapshot it
/// writes until that write returns: two snapshots taken in one order and written in the other would
/// roll one group's cursor back.
#[derive(Debug, Clone, Default)]
pub struct SharedSyncProgress {
    inner: Arc<Mutex<SyncProgress>>,
}

impl SharedSyncProgress {
    pub fn new(progress: SyncProgress) -> Self {
        Self {
            inner: Arc::new(Mutex::new(progress)),
        }
    }

    pub async fn lock(&self) -> MutexGuard<'_, SyncProgress> {
        self.inner.lock().await
    }
}
