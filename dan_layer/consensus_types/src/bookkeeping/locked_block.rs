//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tari_dan_common_types::{Epoch, NodeHeight};

use crate::ids::BlockId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedBlock {
    pub height: NodeHeight,
    pub block_id: BlockId,
    pub epoch: Epoch,
}

impl LockedBlock {
    pub fn height(&self) -> NodeHeight {
        self.height
    }

    pub fn block_id(&self) -> &BlockId {
        &self.block_id
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }
}

impl Display for LockedBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LockedBlock({}, {}, {})", self.epoch, self.height, self.block_id)
    }
}
