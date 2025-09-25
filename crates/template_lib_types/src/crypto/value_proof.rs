//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};

use crate::{
    crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes},
    Amount,
};

/// Proof of knowledge of the opening to a commitment and that the commitment commits to a specific value.
/// Currently used when burning UTXOs to allow the total supply to be adjusted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StealthValueProof {
    /// The claimed value to prove
    pub value: Amount,
    pub knowledge_proof: ValueKnowledgeProof,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ValueKnowledgeProof {
    Commitment {
        /// Signed by C - v.H, where C is the commitment being proven and v is the claimed value
        /// Proving knowledge of the opening to C, and that the commitment C = m.G + v.H
        mask_knowledge_proof: SchnorrSignatureBytes,
    },
    ElgamalEncrypted {
        /// The R.p term of the ElGamal encryption. This allows validators to check the provided value is correct using
        /// the viewable balance. This assumes that the verifiable proof was originally validated correctly.
        reveal_key: RistrettoPublicKeyBytes,
    },
}
