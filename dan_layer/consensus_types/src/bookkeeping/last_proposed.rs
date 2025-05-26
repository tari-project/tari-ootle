//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_dan_common_types::{Epoch, NodeHeight, ShardGroup};

use crate::{BlockId, LeafBlock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastProposed {
    pub height: NodeHeight,
    pub block_id: BlockId,
    pub epoch: Epoch,
    pub shard_group: ShardGroup,
}
impl LastProposed {
    pub fn as_leaf_block(&self) -> LeafBlock {
        LeafBlock {
            block_id: self.block_id,
            height: self.height,
            epoch: self.epoch,
            shard_group: self.shard_group,
        }
    }
}

impl std::fmt::Display for LastProposed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "LastProposed({}, BlockId({}), {})",
            self.height, self.block_id, self.epoch
        )
    }
}
