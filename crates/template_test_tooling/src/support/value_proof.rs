//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_crypto::ristretto::{RistrettoSchnorr, RistrettoSecretKey};
use tari_engine_types::crypto::{commit_amount, messages};
use tari_template_lib::types::{
    Amount,
    crypto::{CommitmentValueProof, ValueKnowledgeProof},
};

/// Proves the value of a commitment by knowledge of its mask. For the view-key path, build the DLEQ proof with
/// `tari_ootle_wallet_crypto::generate_elgamal_value_proof` instead.
pub fn generate_value_proof_mask_knowledge(value: Amount, mask: &RistrettoSecretKey) -> CommitmentValueProof {
    let commitment = commit_amount(mask, value).unwrap();
    let commitment_bytes = commitment.to_byte_type();
    let message = messages::value_proof_message(&commitment_bytes, &value);
    let sig = RistrettoSchnorr::sign(mask, message, &mut rand::rng()).expect("Signing cannot fail");

    CommitmentValueProof {
        value,
        knowledge_proof: ValueKnowledgeProof::Commitment {
            mask_knowledge_proof: sig.to_byte_type(),
        },
    }
}
