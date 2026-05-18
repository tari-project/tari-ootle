//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, NodeHeight};

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct LastSentNewView {
    #[n(0)]
    pub epoch: Epoch,
    #[n(1)]
    pub height: NodeHeight,
}

impl LastSentNewView {
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn height(&self) -> NodeHeight {
        self.height
    }
}

impl std::fmt::Display for LastSentNewView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LastSentNewView({}/{})", self.epoch, self.height)
    }
}
