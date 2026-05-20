//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_ootle_storage::consensus_models::EpochCheckpoint;

use crate::traits::Versioned;

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub enum VersionedEpochCheckpoint {
    #[n(0)]
    V1(#[n(0)] EpochCheckpoint),
}

impl Versioned for VersionedEpochCheckpoint {
    type Latest = EpochCheckpoint;

    fn upgrade_single_step(self) -> (Self, bool) {
        match self {
            Self::V1(_) => (self, false), // No upgrades available
        }
    }

    fn into_latest(self) -> Self::Latest {
        match self {
            Self::V1(checkpoint) => checkpoint,
        }
    }
}

impl From<EpochCheckpoint> for VersionedEpochCheckpoint {
    fn from(checkpoint: EpochCheckpoint) -> Self {
        Self::V1(checkpoint)
    }
}
