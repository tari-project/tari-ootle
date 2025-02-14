//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::consensus_models::{Block, SubstateUpdate};

#[derive(Clone, Debug)]
pub struct BlockData {
    pub block: Block,
    pub diff: Vec<SubstateUpdate>,
}
