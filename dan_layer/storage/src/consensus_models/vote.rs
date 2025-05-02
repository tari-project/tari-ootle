//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::hash::{DefaultHasher, Hash, Hasher};

use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_engine_types::serde_with;
use tari_sidechain::QuorumDecision;

use crate::consensus_models::{BlockId, ValidatorSignature};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub epoch: Epoch,
    pub block_id: BlockId,
    pub block_height: NodeHeight,
    pub decision: QuorumDecision,
    #[serde(with = "serde_with::hex")]
    pub sender_leaf_hash: FixedHash,
    pub signature: ValidatorSignature,
}

impl Vote {
    /// Returns a SIPHASH hash used to uniquely identify this vote
    pub fn get_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }

    pub fn signature(&self) -> &ValidatorSignature {
        &self.signature
    }
}

impl Hash for Vote {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.epoch.hash(state);
        self.block_id.hash(state);
        self.block_height.hash(state);
        // QuorumDecision does not implement Hash
        state.write_u8(self.decision.as_u8());
        self.sender_leaf_hash.hash(state);
        self.signature.hash(state);
    }
}
