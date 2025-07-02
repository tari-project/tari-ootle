//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_ootle_storage::consensus_models::BlockTransactionExecution;

use crate::traits::Versioned;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VersionedBlockTransactionExecution {
    V1(BlockTransactionExecution),
}

impl Versioned for VersionedBlockTransactionExecution {
    type Latest = BlockTransactionExecution;

    fn upgrade_single_step(self) -> (Self, bool) {
        match self {
            Self::V1(_) => (self, false), // No upgrades available
        }
    }

    fn into_latest(self) -> Self::Latest {
        match self {
            Self::V1(execution) => execution,
        }
    }
}

impl From<BlockTransactionExecution> for VersionedBlockTransactionExecution {
    fn from(execution: BlockTransactionExecution) -> Self {
        Self::V1(execution)
    }
}
