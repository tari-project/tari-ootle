//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::ids::BlockId;

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct LastVoted {
    #[n(0)]
    pub block_id: BlockId,
    #[n(1)]
    pub height: NodeHeight,
    #[n(2)]
    pub epoch: Epoch,
}

impl LastVoted {
    pub fn block_id(&self) -> &BlockId {
        &self.block_id
    }

    pub fn height(&self) -> NodeHeight {
        self.height
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }
}

impl std::fmt::Display for LastVoted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "LastVoted(BlockId({}), {}, {})",
            self.block_id, self.height, self.epoch
        )
    }
}
