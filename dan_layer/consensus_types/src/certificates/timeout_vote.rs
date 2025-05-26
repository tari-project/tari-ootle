//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use borsh::BorshSerialize;
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_hashing::layer2::timeout_vote_signature_hasher;
use tari_sidechain::QuorumDecision;
use tari_template_lib::prelude::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

use crate::{validator_signature::ValidatorSignatureBytes, SignedMessage, ToSignatureMessage, Vote};

#[derive(Debug, Clone, serde::Serialize)]
pub struct TimeoutVote {
    pub epoch: Epoch,
    pub height: NodeHeight,
    pub signature: ValidatorSignatureBytes,
}

impl TimeoutVote {
    pub fn signature(&self) -> &ValidatorSignatureBytes {
        &self.signature
    }
}

impl Vote for TimeoutVote {
    type Key = (Epoch, NodeHeight);

    fn key(&self) -> Self::Key {
        (self.epoch, self.height)
    }

    fn epoch(&self) -> Epoch {
        self.epoch
    }

    fn height(&self) -> NodeHeight {
        self.height
    }

    fn decision(&self) -> QuorumDecision {
        QuorumDecision::Accept
    }
}

impl ToSignatureMessage for TimeoutVote {
    fn to_signature_message(&self) -> FixedHash {
        TimeoutVoteMessage {
            epoch: self.epoch,
            height: self.height,
        }
        .to_signature_message()
    }
}

impl SignedMessage for TimeoutVote {
    fn signature(&self) -> &SchnorrSignatureBytes {
        &self.signature.signature
    }

    fn public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.signature.public_key
    }
}

impl Display for TimeoutVote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TimeoutVote {{ epoch: {}, height: {}, signer: {} }}",
            self.epoch, self.height, self.signature.public_key
        )
    }
}

#[derive(BorshSerialize)]
pub struct TimeoutVoteMessage {
    pub epoch: Epoch,
    pub height: NodeHeight,
}

impl ToSignatureMessage for TimeoutVoteMessage {
    fn to_signature_message(&self) -> FixedHash {
        timeout_vote_signature_hasher()
            .chain(&self.epoch)
            .chain(&self.height)
            .finalize()
            .into()
    }
}
