//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::ids::BlockId;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LastExecuted {
    pub height: NodeHeight,
    pub block_id: BlockId,
    pub epoch: Epoch,
}
