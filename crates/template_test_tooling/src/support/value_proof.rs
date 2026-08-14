//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use ootle_byte_type::ToByteType;
use tari_crypto::ristretto::{RistrettoSchnorr, RistrettoSecretKey};
use tari_engine_types::crypto::{commit_amount, messages};
use tari_template_lib::types::{
    Amount,
    crypto::{CommitmentValueProof, PedersenCommitmentBytes, ValueKnowledgeProof},
};

/// The proof map the engine requires to mint or burn a single commitment of `amount` under `mask`, keyed by the
/// commitment as the engine sees it.
pub fn value_proofs_for_commitment<A: Into<Amount>>(
    amount: A,
    mask: &RistrettoSecretKey,
) -> BTreeMap<PedersenCommitmentBytes, CommitmentValueProof> {
    let amount = amount.into();
    let commitment = commit_amount(mask, amount)
        .expect("value_proofs_for_commitment: amount exceeds u64::MAX")
        .to_byte_type();
    BTreeMap::from([(commitment, generate_value_proof_mask_knowledge(amount, mask))])
}

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
