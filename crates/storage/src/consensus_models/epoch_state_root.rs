//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, ShardGroup};
use tari_state_tree::TreeHash;

use crate::{StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};

const LOG_TARGET: &str = "tari::ootle::consensus::epoch_state_root";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochStateRoot {
    /// The epoch that applies to this state root
    pub epoch: Epoch,
    /// The shard group that applies to this state root
    pub shard_group: ShardGroup,
    /// The state root of the epoch
    pub state_root: TreeHash,
}

impl EpochStateRoot {
    pub fn new(epoch: Epoch, shard_group: ShardGroup, state_root: TreeHash) -> Self {
        Self {
            epoch,
            shard_group,
            state_root,
        }
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn shard_group(&self) -> &ShardGroup {
        &self.shard_group
    }

    pub fn state_root(&self) -> TreeHash {
        self.state_root
    }
}

impl EpochStateRoot {
    pub fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        info!(
            target: LOG_TARGET,
            "Setting epoch state root for epoch {} and shard group {} to {}",
            self.epoch,
            self.shard_group,
            self.state_root,
        );
        tx.previous_epoch_state_root_set(self)
    }

    pub fn get<TTx: StateStoreReadTransaction>(tx: &TTx) -> Result<Self, StorageError> {
        tx.previous_epoch_state_root_get()
    }
}
