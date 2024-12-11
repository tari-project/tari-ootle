//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use serde::Serialize;
use tari_dan_storage::consensus_models::{Block, ForeignProposal};

#[derive(Debug, Clone, Serialize)]
pub struct ProposalMessage {
    pub block: Block,
    pub foreign_proposals: Vec<ForeignProposal>,
}

impl Display for ProposalMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProposalMessage({})", self.block)
    }
}
