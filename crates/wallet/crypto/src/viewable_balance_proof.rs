//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use chacha20poly1305::aead::OsRng;
use ootle_byte_type::ToByteType;
use tari_crypto::{
    commitment::HomomorphicCommitmentFactory,
    keys::{PublicKey, SecretKey},
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};
use tari_engine_types::crypto::{get_commitment_factory, messages};
use tari_template_lib_types::{
    crypto::Scalar32Bytes,
    stealth::{ViewableBalanceProof, ViewableBalanceProofChallengeFields},
};
use tari_utilities::ByteArray;

use crate::ConfidentialProofError;

pub fn create_viewable_balance_proof(
    mask: &RistrettoSecretKey,
    output_amount: u64,
    commitment: &PedersenCommitment,
    view_key: &RistrettoPublicKey,
) -> Result<ViewableBalanceProof, ConfidentialProofError> {
    let (elgamal_secret_nonce, elgamal_public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let r = &elgamal_secret_nonce;
    let output_amount_as_secret = RistrettoSecretKey::from(output_amount);

    // E = v.G + rP
    let elgamal_encrypted = RistrettoPublicKey::from_secret_key(&output_amount_as_secret) + r * view_key;

    // Nonces
    let x_v = RistrettoSecretKey::random(&mut OsRng);
    let x_m = RistrettoSecretKey::random(&mut OsRng);
    let x_r = RistrettoSecretKey::random(&mut OsRng);

    // C' = x_m.G + x_v.H
    let c_prime = get_commitment_factory().commit(&x_m, &x_v);
    // E' = x_v.G + x_r.P
    let e_prime = RistrettoPublicKey::from_secret_key(&x_v) + &x_r * view_key;
    // R' = x_r.G
    let r_prime = RistrettoPublicKey::from_secret_key(&x_r);

    // Create challenge
    let elgamal_encrypted = elgamal_encrypted.to_byte_type();
    let elgamal_public_nonce = elgamal_public_nonce.to_byte_type();
    let c_prime = c_prime.to_byte_type();
    let e_prime = e_prime.to_byte_type();
    let r_prime = r_prime.to_byte_type();

    let challenge_fields = ViewableBalanceProofChallengeFields {
        elgamal_encrypted: &elgamal_encrypted,
        elgamal_public_nonce: &elgamal_public_nonce,
        c_prime: &c_prime,
        e_prime: &e_prime,
        r_prime: &r_prime,
    };

    let e = messages::viewable_balance_proof64(commitment, view_key, challenge_fields);

    // Generate signatures
    // TODO: sign_raw_uniform should take a [u8; 64] for the challenge so that length mismatches are caught at compile
    //       time. The challenge is never a secret (in all current usages), so non-zeroed memory is not an issue.

    // sv = ev + x_v
    let s_v = RistrettoSchnorr::sign_raw_uniform(&output_amount_as_secret, x_v, &e)
        .expect("INVARIANT VIOLATION: sv RistrettoSchnorr::sign_raw_uniform and challenge hash output length mismatch");
    // sm = em + x_m
    let s_m = RistrettoSchnorr::sign_raw_uniform(mask, x_m, &e)
        .expect("INVARIANT VIOLATION: sm RistrettoSchnorr::sign_raw_uniform and challenge hash output length mismatch");
    // sr = er + x_r
    let s_r = RistrettoSchnorr::sign_raw_uniform(r, x_r, &e)
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
