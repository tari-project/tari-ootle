//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::Serialize;
use tari_ootle_common_types::{Epoch, NodeHeight};

#[derive(Debug, Clone, Serialize)]
pub struct SyncRequestMessage {
    pub epoch: Epoch,
    pub block_height: NodeHeight,
}
