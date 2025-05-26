//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_dan_common_types::{Epoch, NodeHeight};

use crate::{BlockId, ProposalVote};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastSentVote {
    pub vote: ProposalVote,
}

impl LastSentVote {
    pub fn epoch(&self) -> Epoch {
        self.vote.epoch
    }

    pub fn block_id(&self) -> BlockId {
        self.vote.block_id
    }

    pub fn block_height(&self) -> NodeHeight {
        self.vote.block_height
    }
}

impl std::fmt::Display for LastSentVote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LastVote({})", self.vote)
    }
}

impl From<ProposalVote> for LastSentVote {
    fn from(vote: ProposalVote) -> Self {
        Self { vote }
    }
}
