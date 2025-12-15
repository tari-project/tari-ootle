//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::FixedHash;
use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_sidechain::QuorumDecision;
use tari_template_lib::prelude::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

pub trait ToSignatureMessage {
    fn to_signature_message(&self) -> FixedHash;
}

pub trait SignedMessage: ToSignatureMessage {
    fn signature(&self) -> &SchnorrSignatureBytes;
    fn public_key(&self) -> &RistrettoPublicKeyBytes;
}

pub trait Vote: SignedMessage {
    fn epoch(&self) -> Epoch;
    fn height(&self) -> NodeHeight;
    fn decision(&self) -> QuorumDecision;
}
