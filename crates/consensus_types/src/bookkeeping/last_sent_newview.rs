//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::SignedTimeout;

/// The view this node's last NEWVIEW named, and the timeout votes it signed for that view.
///
/// The votes are stored so that a restarted node re-sends the signatures it sent before. A member that already holds
/// one of them recognises a re-send, and a vote signed afresh for the same view would be indistinguishable from an
/// answer.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct LastSentNewView {
    #[n(0)]
    pub epoch: Epoch,
    #[n(1)]
    pub height: NodeHeight,
    /// The vote sent to the committee members other than the leader. `None` in a record written before votes were
    /// stored.
    #[n(2)]
    pub to_members: Option<SignedTimeout>,
    /// The vote in the NEWVIEW sent to the leader. `None` in a record written before votes were stored.
    #[n(3)]
    pub to_leader: Option<SignedTimeout>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Encode)]
    struct LastSentNewViewWithoutVotes {
        #[n(0)]
        epoch: Epoch,
        #[n(1)]
        height: NodeHeight,
    }

    #[test]
    fn a_record_without_votes_decodes_with_none() {
        let encoded = minicbor::to_vec(LastSentNewViewWithoutVotes {
            epoch: Epoch(3),
            height: NodeHeight(12),
        })
        .unwrap();
        let decoded: LastSentNewView = minicbor::decode(&encoded).unwrap();
        assert_eq!(decoded.epoch, Epoch(3));
        assert_eq!(decoded.height, NodeHeight(12));
        assert!(decoded.to_members.is_none());
        assert!(decoded.to_leader.is_none());
    }
}
