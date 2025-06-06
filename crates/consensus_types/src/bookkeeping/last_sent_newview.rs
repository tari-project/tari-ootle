//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, NodeHeight};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastSentNewView {
    pub epoch: Epoch,
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
