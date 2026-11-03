//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::{Seq, serde_as};
use tari_ootle_common_types::{Epoch, ShardGroup, StateVersion, shard::Shard};
use tari_state_tree::TransitionAccumulator;
use tokio::sync::{Mutex, MutexGuard};

use crate::network_state_sync::verified_stage::AccumulatedVersion;

#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncProgress {
    pub last_epoch: Epoch,
    // These transform the maps into Vecs for JSON serialization (can't have non-string keys).
    #[serde_as(as = "Seq<(_, _)>")]
    pub checkpoint_progress: IndexMap<ShardGroup, Epoch>,
    /// The last version staged of each shard, where its stream resumes.
    #[serde_as(as = "Seq<(_, _)>")]
    pub last_state_versions: IndexMap<Shard, (StateVersion, Epoch)>,
    /// The last version of each shard a verified leaf authenticated, which every version up to is promoted to the
    /// indexed tables through.
    #[serde_as(as = "Seq<(_, _)>")]
    pub verified: IndexMap<Shard, VerifiedShardVersion>,
}

/// A shard's state version, with the epoch it was committed at and the transition accumulator after it, as a verified
/// leaf authenticated it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedShardVersion {
    pub state_version: StateVersion,
    pub epoch: Epoch,
    pub transition_acc: TransitionAccumulator,
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

    /// `shard`'s last verified version and the accumulator after it, or `None` for a shard nothing is verified of.
    pub fn verified(&self, shard: Shard) -> Option<AccumulatedVersion> {
        self.verified.get(&shard).map(|verified| AccumulatedVersion {
            version: verified.state_version,
            acc: verified.transition_acc,
        })
    }

    /// `shard`'s last verified version, or `None` for a shard nothing is verified of.
    pub fn verified_version(&self, shard: Shard) -> Option<StateVersion> {
        self.verified.get(&shard).map(|verified| verified.state_version)
    }

    pub fn record_verified(&mut self, shard: Shard, verified: VerifiedShardVersion) {
        self.verified.insert(shard, verified);
    }

    /// Moves `shard`'s stream cursor back to its last verified version, discarding what was staged above it.
    pub fn rewind_to_verified(&mut self, shard: Shard) {
        match self.verified.get(&shard) {
            Some(verified) => {
                self.last_state_versions
                    .insert(shard, (verified.state_version, verified.epoch));
            },
            None => {
                self.last_state_versions.shift_remove(&shard);
            },
        }
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
