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
        self.timeout.epoch() == self.high_pc.epoch() && self.timeout.high_pc_height == self.high_pc.height()
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

#[cfg(test)]
mod tests {
    use tari_consensus_types::{BlockId, QuorumDecision, ValidatorSignatureBytes};
    use tari_ootle_common_types::{Epoch, NodeHeight, NumPreshards, ShardGroup, StateTreeHash};
    use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

    use super::*;
    use crate::messages::HotstuffMessage;

    fn sample_high_pc(epoch: Epoch, height: NodeHeight) -> ProposalCertificate {
        ProposalCertificate::new(
            StateTreeHash::default(),
            BlockId::zero(),
            height,
            epoch,
            ShardGroup::all_shards(NumPreshards::P64),
            vec![],
            QuorumDecision::Accept,
        )
    }

    fn sample_timeout_vote(epoch: Epoch, height: NodeHeight, high_pc_height: NodeHeight) -> TimeoutVote {
        TimeoutVote {
            epoch,
            height,
            high_pc_height,
            signature: ValidatorSignatureBytes::new(
                RistrettoPublicKeyBytes::default(),
                SchnorrSignatureBytes::zero(),
            ),
        }
    }

    #[test]
    fn timeout_claim_requires_matching_epoch_and_height() {
        let msg = NewViewMessage {
            high_pc: sample_high_pc(Epoch(2), NodeHeight(5)),
            last_vote: None,
            timeout: sample_timeout_vote(Epoch(2), NodeHeight(6), NodeHeight(5)),
        };
        assert!(msg.timeout_claim_matches_high_pc());

        let mismatched_epoch = NewViewMessage {
            high_pc: sample_high_pc(Epoch(1), NodeHeight(5)),
            last_vote: None,
            timeout: sample_timeout_vote(Epoch(2), NodeHeight(6), NodeHeight(5)),
        };
        assert!(!mismatched_epoch.timeout_claim_matches_high_pc());
        assert_eq!(
            HotstuffMessage::new_newview(mismatched_epoch.clone()).epoch(),
            mismatched_epoch.epoch()
        );
    }
}
