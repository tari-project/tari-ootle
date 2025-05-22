//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use tari_consensus_types::{ProposalCertificate, ProposalVote, TimeoutVote, Vote};
use tari_dan_common_types::{displayable::Displayable, Epoch, NodeHeight};

#[derive(Debug, Clone, serde::Serialize)]
pub struct NewViewMessage {
    pub high_pc: ProposalCertificate,
    pub last_vote: Option<ProposalVote>,
    pub timeout: TimeoutVote,
}

impl NewViewMessage {
    pub fn max_height(&self) -> NodeHeight {
        self.high_pc.height().max(self.timeout.height()).max(
            self.last_vote
                .as_ref()
                .map(|v| v.height())
                .unwrap_or_else(NodeHeight::zero),
        )
    }

    pub fn epoch(&self) -> Epoch {
        // We'll take the epoch from the timeout vote arbitrarily. Epoch should be validated to all match.
        self.timeout.epoch()
    }
}

impl Display for NewViewMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "NewViewMessage {{ high_pc: {}, last_vote: {}, timeout: {} }}",
            self.high_pc,
            self.last_vote.display(),
            self.timeout
        )
    }
}
