//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_consensus_types::ProposalVote;
use tari_ootle_common_types::{Epoch, NodeHeight, diagnostics::unix_millis_now};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

/// Evidence that one validator signed two votes attesting to different things for the same view.
///
/// Both votes are kept whole so the record stands on its own: a third party can rebuild each signed
/// preimage from it and check the signatures without consulting any other state. Both were
/// signature-checked against the signer before the record was built, so the pair is proof of
/// misbehaviour rather than a report of it.
///
/// Only proposal votes can produce this. A timeout vote's preimage is `(epoch, height)` and nothing
/// else, which is exactly the view it is bucketed under, so two timeout votes from one signer at one
/// view necessarily attest to the same thing; they can differ only in the signature's nonce, which
/// any signer can vary at will.
///
/// Nothing consumes it yet — there is no in-protocol penalty — so it is written for operators and
/// for a future slashing path.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct VoteEquivocation {
    #[n(0)]
    pub epoch: Epoch,
    #[n(1)]
    pub height: NodeHeight,
    #[n(2)]
    pub public_key: RistrettoPublicKeyBytes,
    /// Unix milliseconds at which this node noticed the second vote.
    #[n(3)]
    pub detected_at: u64,
    #[n(4)]
    pub first: ProposalVote,
    #[n(5)]
    pub second: ProposalVote,
}

impl VoteEquivocation {
    /// Builds evidence from two proposal votes by one signer for one view, or `None` when the two
    /// attest to the same `(block_id, decision)`.
    ///
    /// Two valid signatures over one message are not equivocation: signing draws a fresh nonce, so a
    /// signer can produce arbitrarily many, and each says exactly what the others do. What makes a
    /// pair evidence is a difference in what is attested.
    pub fn from_conflicting_votes(
        epoch: Epoch,
        height: NodeHeight,
        public_key: RistrettoPublicKeyBytes,
        first: ProposalVote,
        second: ProposalVote,
    ) -> Option<Self> {
        if first.block_id == second.block_id && first.decision == second.decision {
            return None;
        }

        Some(Self {
            epoch,
            height,
            public_key,
            detected_at: unix_millis_now(),
            first,
            second,
        })
    }
}

impl Display for VoteEquivocation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "vote equivocation by {} at {}/{}: {:?} for {} and {:?} for {}",
            self.public_key,
            self.epoch,
            self.height,
            self.first.decision,
            self.first.block_id,
            self.second.decision,
            self.second.block_id
        )
    }
}
