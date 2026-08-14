//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};

use crate::{
    Amount,
    crypto::{RistrettoPublicKeyBytes, Scalar32Bytes, SchnorrSignatureBytes},
};

/// Proof of knowledge of the opening to a commitment and that the commitment commits to a specific value.
///
/// Used wherever a resource's total supply must account for a value that is otherwise hidden in a commitment:
/// burning a stealth UTXO, and minting a confidential commitment.
#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CommitmentValueProof {
    /// The claimed value to prove
    #[n(0)]
    pub value: Amount,
    #[n(1)]
    pub knowledge_proof: ValueKnowledgeProof,
}

#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ValueKnowledgeProof {
    #[n(0)]
    Commitment {
        /// Signed by C - v.H, where C is the commitment being proven and v is the claimed value
        /// Proving knowledge of the opening to C, and that the commitment C = m.G + v.H
        #[n(0)]
        mask_knowledge_proof: SchnorrSignatureBytes,
    },
    #[n(1)]
    ElgamalEncrypted {
        /// Chaum-Pedersen (DLEQ) proof of knowledge of the view private key `p` such that `P = p.G` (the resource
        /// view key) and `E - v.G = p.R`, where `(E, R)` is the UTXO's viewable balance and `v` is the claimed value.
        /// Binding both equations to a single scalar forces `v` to equal the value encrypted for the view key.
        ///
        /// `K_g = k.G` for the proof nonce `k`
        #[n(0)]
        public_nonce_g: RistrettoPublicKeyBytes,
        /// `K_r = k.R`
        #[n(1)]
        public_nonce_r: RistrettoPublicKeyBytes,
        /// `s = k + e.p` for the Fiat-Shamir challenge `e`
        #[n(2)]
        s_p: Scalar32Bytes,
    },
}
