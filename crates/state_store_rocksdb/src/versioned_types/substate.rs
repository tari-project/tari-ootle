//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_ootle_storage::consensus_models::SubstateRecord;

use crate::traits::Versioned;

pub type LatestSubstateRecord = SubstateRecord;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VersionedSubstateRecord {
    V1(SubstateRecord),
}

impl Versioned for VersionedSubstateRecord {
    type Latest = LatestSubstateRecord;

    fn upgrade_single_step(self) -> (Self, bool) {
        match self {
            Self::V1(_) => (self, false), // No upgrades available
        }
    }

    fn into_latest(self) -> Self::Latest {
        match self {
            Self::V1(record) => record,
        }
    }
}

impl From<SubstateRecord> for VersionedSubstateRecord {
    fn from(record: SubstateRecord) -> Self {
        Self::V1(record)
    }
}
