//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_epoch_manager::service::NetworkDescription;
use tari_ootle_common_types::{shard::Shard, Epoch, ShardGroup, StateVersion};

use crate::network_state_sync::{committee_client::ValidatorCommitteeRpcPool, sync_progress::SyncProgress};

pub struct SyncPlan {
    network_description: NetworkDescription,
    sync_progress: SyncProgress,
    committee_pools: HashMap<ShardGroup, ValidatorCommitteeRpcPool>,
}

impl SyncPlan {
    pub fn new(
        network_description: NetworkDescription,
        sync_progress: SyncProgress,
        committee_pools: HashMap<ShardGroup, ValidatorCommitteeRpcPool>,
    ) -> Self {
        Self {
            network_description,
            sync_progress,
            committee_pools,
        }
    }

    pub fn add_checkpoint_sync_progress(&mut self, shard_group: ShardGroup, epoch: Epoch) {
        self.sync_progress.checkpoint_progress.insert(shard_group, epoch);
    }

    pub fn add_state_sync_progress(&mut self, shard: Shard, state_version: StateVersion, epoch: Epoch) {
        self.sync_progress
            .last_state_versions
            .insert(shard, (state_version, epoch));
    }

    pub fn sync_progress(&self) -> &SyncProgress {
        &self.sync_progress
    }

    pub fn committee_pools(&self) -> &HashMap<ShardGroup, ValidatorCommitteeRpcPool> {
        &self.committee_pools
    }

    pub fn network_description(&self) -> &NetworkDescription {
        &self.network_description
    }
}
