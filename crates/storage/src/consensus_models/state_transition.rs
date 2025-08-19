//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::num::NonZeroUsize;

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
    pub fn into_chunks(self, size: NonZeroUsize) -> Vec<Self> {
        let num_chunks = self.updates.len().div_ceil(size.get());
        let mut chunks = Vec::with_capacity(num_chunks);
        let mut updates = self.updates;
        while !updates.is_empty() {
            let take = updates.len().min(size.get());
            let chunk_updates: Vec<_> = updates.drain(..take).collect();
            chunks.push(Self {
                epoch: self.epoch,
                shard: self.shard,
                state_version: self.state_version,
                updates: chunk_updates,
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
