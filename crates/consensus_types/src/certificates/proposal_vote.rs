//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_hashing::layer2;
use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_sidechain::{ProposalCertificateSignatureFields, QuorumDecision};
use tari_template_lib::types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

use crate::{SignedMessage, ToSignatureMessage, Vote, ids::BlockId, validator_signature::ValidatorSignatureBytes};

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct ProposalVote {
    #[n(0)]
    pub epoch: Epoch,
    #[n(1)]
    pub block_id: BlockId,
    /// The height of the view change - this should correspond to the height of the block.
    /// NOTE: that this is not validated explicitly and is mainly used to determine message age and ordering.
    #[n(2)]
    pub block_height: NodeHeight,
    // QuorumDecision is foreign (tari_sidechain) — bridge through serde.
    #[n(3)]
    #[cbor(with = "tari_bor::adapters::serde_bridge")]
    pub decision: QuorumDecision,
    #[n(4)]
    pub signature: ValidatorSignatureBytes,
}

impl Vote for ProposalVote {
    fn epoch(&self) -> Epoch {
        self.epoch
    }

    fn height(&self) -> NodeHeight {
        self.block_height
    }

    fn decision(&self) -> QuorumDecision {
        self.decision
    }
}

impl ToSignatureMessage for ProposalVote {
    fn to_signature_message(&self) -> FixedHash {
        ProposalCertificateSignatureFields {
            block_id: self.block_id.hash(),
            decision: self.decision,
        }
        .to_signature_message()
    }
}

impl SignedMessage for ProposalVote {
    fn signature(&self) -> &SchnorrSignatureBytes {
        &self.signature.signature
    }

    fn public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.signature.public_key
    }
}

impl Display for ProposalVote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ProposalVote: {}/{}, block_id: {}, decision: {}, voter: {}",
            self.epoch, self.block_height, self.block_id, self.decision, self.signature.public_key
        )
    }
}

impl ToSignatureMessage for ProposalCertificateSignatureFields<'_> {
    /// Defines the signature message for a proposal vote which is collected into a ProposalCertificate.
    fn to_signature_message(&self) -> FixedHash {
        layer2::proposal_vote_signature_hasher().chain(self).finalize().into()
    }
}
