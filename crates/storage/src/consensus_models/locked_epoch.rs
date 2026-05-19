//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use minicbor::{CborLen, Decode, Encode};
use tari_ootle_common_types::Epoch;
use tari_template_lib_types::Hash32;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Encode, Decode, CborLen)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LockedEpoch {
    #[n(0)]
    epoch: Epoch,
    #[n(1)]
    hash: Hash32,
}

impl LockedEpoch {
    pub fn new(epoch: Epoch, hash: Hash32) -> Self {
        Self { epoch, hash }
    }

    pub fn hash(&self) -> &Hash32 {
        &self.hash
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn destructure(self) -> (Epoch, Hash32) {
        (self.epoch, self.hash)
    }
}

impl Display for LockedEpoch {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "LockedEpoch({}, {})", self.epoch, self.hash)
    }
}
