//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_crypto::{
    commitment::HomomorphicCommitmentFactory,
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey, pedersen::PedersenCommitment},
};
use tari_engine_types::crypto::{ElgamalVerifiableBalance, get_commitment_factory, messages};
use tari_template_lib_types::{
    Amount,
    crypto::{PedersenCommitmentBytes, Scalar32Bytes, StealthValueProof, ValueKnowledgeProof},
    stealth::{ViewableBalanceProof, ViewableBalanceProofMessageFields},
};
use tari_utilities::ByteArray;

use crate::StealthProofError;

pub fn generate_elgamal_viewable_balance_proof(
    mask: &RistrettoSecretKey,
    output_amount: u64,
    commitment: &PedersenCommitment,
    view_key: &RistrettoPublicKey,
) -> Result<ViewableBalanceProof, StealthProofError> {
    let mut rng = rand::rng();
    let elgamal_secret_nonce = RistrettoSecretKey::random(&mut rng);
    let x_v = RistrettoSecretKey::random(&mut rng);
    let x_m = RistrettoSecretKey::random(&mut rng);
    let x_r = RistrettoSecretKey::random(&mut rng);
    generate_elgamal_viewable_balance_proof_seeded(
        mask,
        output_amount,
        commitment,
        view_key,
        &elgamal_secret_nonce,
        &x_v,
        &x_m,
        &x_r,
    )
}

/// Like [`generate_elgamal_viewable_balance_proof`], but the four ZK nonces are supplied by the
/// caller (the ElGamal ephemeral nonce `r` plus the proof nonces `x_v`, `x_m`, `x_r`) instead of
/// being drawn at random. With fixed nonces the produced proof is deterministic and reproducible,
/// which is intended for deterministic builders and test vectors. The proof is otherwise identical
/// to the random-nonce path.
#[allow(clippy::too_many_arguments)]
pub fn generate_elgamal_viewable_balance_proof_seeded(
    mask: &RistrettoSecretKey,
    output_amount: u64,
    commitment: &PedersenCommitment,
    view_key: &RistrettoPublicKey,
    elgamal_secret_nonce: &RistrettoSecretKey,
    x_v: &RistrettoSecretKey,
    x_m: &RistrettoSecretKey,
    x_r: &RistrettoSecretKey,
) -> Result<ViewableBalanceProof, StealthProofError> {
    let elgamal_public_nonce = RistrettoPublicKey::from_secret_key(elgamal_secret_nonce);
    let r = elgamal_secret_nonce;
    let output_amount_as_secret = RistrettoSecretKey::from(output_amount);

    // E = v.G + rP
    let elgamal_encrypted = RistrettoPublicKey::from_secret_key(&output_amount_as_secret) + r * view_key;

    // C' = x_m.G + x_v.H
    let c_prime = get_commitment_factory().commit(x_m, x_v);
    // E' = x_v.G + x_r.P
    let e_prime = RistrettoPublicKey::from_secret_key(x_v) + x_r * view_key;
    // R' = x_r.G
    let r_prime = RistrettoPublicKey::from_secret_key(x_r);

    // Create challenge
    let elgamal_encrypted = elgamal_encrypted.to_byte_type();
    let elgamal_public_nonce = elgamal_public_nonce.to_byte_type();
    let c_prime = c_prime.to_byte_type();
    let e_prime = e_prime.to_byte_type();
    let r_prime = r_prime.to_byte_type();

    let message_fields = ViewableBalanceProofMessageFields {
        elgamal_encrypted: &elgamal_encrypted,
        elgamal_public_nonce: &elgamal_public_nonce,
        c_prime: &c_prime,
        e_prime: &e_prime,
        r_prime: &r_prime,
    };

    let e = messages::viewable_balance_proof64(commitment, view_key, message_fields);

    // Generate signatures
    // TODO: sign_raw_uniform should take a [u8; 64] for the challenge so that length mismatches are caught at compile
    //       time. The challenge is never a secret (in all current usages), so non-zeroed memory is not an issue.

    // sv = ev + x_v
    let s_v = RistrettoSchnorr::sign_raw_uniform(&output_amount_as_secret, x_v.clone(), &e)
        .expect("INVARIANT VIOLATION: sv RistrettoSchnorr::sign_raw_uniform and challenge hash output length mismatch");
    // sm = em + x_m
    let s_m = RistrettoSchnorr::sign_raw_uniform(mask, x_m.clone(), &e)
        .expect("INVARIANT VIOLATION: sm RistrettoSchnorr::sign_raw_uniform and challenge hash output length mismatch");
    // sr = er + x_r
    let s_r = RistrettoSchnorr::sign_raw_uniform(r, x_r.clone(), &e)
        .expect("INVARIANT VIOLATION: sr RistrettoSchnorr::sign_raw_uniform and challenge hash output length mismatch");

    Ok(ViewableBalanceProof {
        elgamal_encrypted,
        elgamal_public_nonce,
        c_prime,
        e_prime,
        r_prime,
        s_v: Scalar32Bytes::from_bytes(s_v.get_signature().as_bytes())
            .expect("INVARIANT VIOLATION: s_v length mismatch"),
        s_m: Scalar32Bytes::from_bytes(s_m.get_signature().as_bytes())
            .expect("INVARIANT VIOLATION: s_m length mismatch"),
        s_r: Scalar32Bytes::from_bytes(s_r.get_signature().as_bytes())
            .expect("INVARIANT VIOLATION: s_r length mismatch"),
    })
}

/// Generates a [`StealthValueProof`] for the value encrypted in a UTXO's viewable balance. The prover is the
/// view-key holder: the proof is a Chaum-Pedersen DLEQ demonstrating knowledge of the view private key `p` such
/// that `P = p.G` and `E - v.G = p.R`, which holds only when `v` is the value encrypted for the view key.
pub fn generate_elgamal_value_proof(
    view_private_key: &RistrettoSecretKey,
    value: u64,
    commitment: &PedersenCommitmentBytes,
    verifiable_balance: &ElgamalVerifiableBalance,
) -> StealthValueProof {
    let mut rng = rand::rng();
    let value = Amount::from(value);
    let view_key = RistrettoPublicKey::from_secret_key(view_private_key);

    let k = RistrettoSecretKey::random(&mut rng);
    let public_nonce_g = RistrettoPublicKey::from_secret_key(&k);
    let public_nonce_r = &k * &verifiable_balance.public_nonce;

    let challenge = messages::elgamal_value_proof64(
        commitment,
        &view_key,
        &verifiable_balance.encrypted,
        &verifiable_balance.public_nonce,
        &value,
        &public_nonce_g,
        &public_nonce_r,
    );
    let e = RistrettoSecretKey::from_uniform_bytes(&challenge)
        .expect("INVARIANT VIOLATION: RistrettoSecretKey::from_uniform_bytes and challenge hash length mismatch");

    // s = k + e.p
    let s_p = &k + &e * view_private_key;

    StealthValueProof {
        value,
        knowledge_proof: ValueKnowledgeProof::ElgamalEncrypted {
            public_nonce_g: public_nonce_g.to_byte_type(),
            public_nonce_r: public_nonce_r.to_byte_type(),
            s_p: Scalar32Bytes::from_bytes(s_p.as_bytes()).expect("INVARIANT VIOLATION: s_p length mismatch"),
        },
    }
}
