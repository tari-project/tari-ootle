//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use indexmap::IndexMap;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{shard::Shard, Epoch, VersionedSubstateId};
use tari_state_tree::Version;

use crate::consensus_models::SubstateValueOrHash;

pub struct SubstateUpdateBatch {
    pub epoch: Epoch,
    pub updates: IndexMap<Shard, IndexMap<Version, Vec<SubstateTransition>>>,
}

impl SubstateUpdateBatch {
    pub fn new(epoch: Epoch) -> Self {
        Self {
            epoch,
            updates: IndexMap::new(),
        }
    }

    pub fn with_transition(&mut self, shard: Shard, state_version: Version) -> &mut Vec<SubstateTransition> {
        self.updates.entry(shard).or_default().entry(state_version).or_default()
    }
}

pub enum SubstateTransition {
    Up {
        id: SubstateId,
        version: u32,
        substate_or_hash: SubstateValueOrHash,
    },
    Down {
        id: VersionedSubstateId,
    },
}
