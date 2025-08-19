//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{shard::Shard, Epoch};
use tari_state_tree::Version;

use crate::{consensus_models::SubstateUpdateProof, StateStoreReadTransaction, StorageError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateVersionTransitions {
    pub epoch: Epoch,
    pub shard: Shard,
    pub state_version: Version,
    pub updates: Vec<SubstateUpdateProof>,
}

impl StateVersionTransitions {
    pub fn into_chunks(mut self, size: usize) -> Vec<Self> {
        let num_chunks = self.updates.len().div_ceil(size);
        let mut chunks = Vec::with_capacity(num_chunks);
        loop {
            if self.updates.len() < size {
                chunks.push(Self {
                    epoch: self.epoch,
                    shard: self.shard,
                    state_version: self.state_version,
                    updates: self.updates,
                });
                break;
            }

            let chunk = self.updates.split_off(size);

            chunks.push(Self {
                epoch: self.epoch,
                shard: self.shard,
                state_version: self.state_version,
                updates: chunk,
            });
        }
        chunks
    }
}

pub struct StateTransition;

impl StateTransition {
    pub fn get_for_shard<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        shard: Shard,
        state_version: Version,
        include_values: bool,
    ) -> Result<StateVersionTransitions, StorageError> {
        tx.state_transitions_get_after(shard, state_version, include_values)
    }
}
