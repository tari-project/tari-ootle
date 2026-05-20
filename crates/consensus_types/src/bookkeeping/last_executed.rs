//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::ids::BlockId;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct LastExecuted {
    #[n(0)]
    pub height: NodeHeight,
    #[n(1)]
    pub block_id: BlockId,
    #[n(2)]
    pub epoch: Epoch,
}
