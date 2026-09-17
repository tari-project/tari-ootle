//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use tari_consensus_types::{ProposalCertificate, ProposalVote, TimeoutVote, Vote};
use tari_ootle_common_types::{Epoch, NodeHeight, displayable::Displayable};

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

    /// Whether the height the timeout vote signs is the height of the certificate the message carries.
    ///
    /// The signed height travels on into a timeout certificate that binds the next leader, and every replica but
    /// the one that receives this message has to take that signature on trust. The claim is only worth the
    /// certificate carried beside it.
    pub fn timeout_claim_matches_high_pc(&self) -> bool {
        self.timeout.high_pc_height == self.high_pc.height()
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
