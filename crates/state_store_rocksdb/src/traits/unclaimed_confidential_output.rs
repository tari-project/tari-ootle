//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_engine_types::confidential::UnclaimedConfidentialOutput;

use crate::traits::Versioned;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VersionedUnclaimedConfidentialOutput {
    V1(UnclaimedConfidentialOutput),
}

impl Versioned for VersionedUnclaimedConfidentialOutput {
    type Latest = UnclaimedConfidentialOutput;

    fn upgrade_single_step(self) -> (Self, bool) {
        match self {
            Self::V1(_) => (self, false), // No upgrades available
        }
    }

    fn into_latest(self) -> Self::Latest {
        match self {
            Self::V1(output) => output,
        }
    }
}

impl From<UnclaimedConfidentialOutput> for VersionedUnclaimedConfidentialOutput {
    fn from(output: UnclaimedConfidentialOutput) -> Self {
        Self::V1(output)
    }
}
