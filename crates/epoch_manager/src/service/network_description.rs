//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use tari_ootle_common_types::{Epoch, NumPreshards, ShardGroup};

#[derive(Debug, Clone)]
pub struct NetworkDescription {
    pub epoch: Epoch,
    /// Ordered map of shard groups and their information.
    pub shard_groups: BTreeMap<ShardGroup, ShardGroupInfo>,
    pub num_preshards: NumPreshards,
}

impl NetworkDescription {
    pub fn get_shard_group_info(&self, shard_group: &ShardGroup) -> Option<&ShardGroupInfo> {
        self.shard_groups.get(shard_group)
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn shard_groups_iter(&self) -> impl Iterator<Item = ShardGroup> + '_ {
        self.shard_groups.keys().copied()
    }

    pub fn num_committees(&self) -> usize {
        self.shard_groups.len()
    }
}

#[derive(Debug, Clone)]
pub struct ShardGroupInfo {
    pub num_members: u32,
}
