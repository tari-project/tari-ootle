//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Host reference for the on-device signing recipe (`ootle-ledger-app/src/hashing.rs`).
//!
//! This independently reimplements, using the `blake2` crate + `curve25519-dalek`, the exact byte
//! recipe the Ledger device follows to (1) recompute a transaction's 64-byte signing message from
//! the streamed preimage segments and (2) produce a Ristretto Schnorr signature. It then proves
//! the result against `tari_ootle_transaction` / `tari_crypto`:
//!   - the reference message equals `create_message_v1` (domain separation + borsh feeding), and
//!   - the reference signature verifies via `verify_v1` (Schnorr challenge + `s = e*k + r`).
//!
//! If this passes, the device recipe is correct by construction; only the SDK's Blake2b and the
//! APDU transport remain to be exercised on Speculos.

use blake2::{Blake2b512, Digest};
use curve25519_dalek::{Scalar, constants::RISTRETTO_BASEPOINT_TABLE};
use indexmap::IndexSet;
use ootle_ledger_common::signing::{
    NONCE_DOMAIN,
    SCHNORR_DOMAIN,
    SCHNORR_DOMAIN_VERSION,
    SCHNORR_LABEL,
    TX_DOMAIN,
    TX_DOMAIN_VERSION,
    TX_LABEL_SEAL,
    TX_LABEL_SIGNATURE,
};
use rand::Rng;
use tari_ootle_transaction::{
    Blob,
    Blobs,
    Instruction,
    PreimageSegment,
    TransactionSealSignature,
    TransactionSignature,
    UnsealedTransactionV1,
    UnsignedTransactionV1,
};
use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

// ---- reference reimplementation of the device recipe (mirrors src/hashing.rs) -------------------

fn byte_to_decimal_ascii_bytes(mut byte: u8) -> (usize, [u8; 3]) {
    const ZERO: u8 = 48;
    let mut bytes = [0u8, 0u8, ZERO];
    let mut pos = 3usize;
    if byte == 0 {
        return (2, bytes);
    }
    while byte > 0 {
        bytes[pos - 1] = ZERO + (byte % 10);
        byte /= 10;
        pos -= 1;
    }
    (pos, bytes)
}

fn add_domain_separation_tag(h: &mut Blake2b512, domain: &str, version: u8, label: &str) {
    let (off, ascii) = byte_to_decimal_ascii_bytes(version);
    let version_len = 3 - off;
    let len = if label.is_empty() {
        domain.len() + version_len + 2
    } else {
        domain.len() + version_len + label.len() + 3
    };
    h.update((len as u64).to_le_bytes());
    h.update(domain.as_bytes());
    h.update(b".v");
    h.update(&ascii[off..]);
    if !label.is_empty() {
        h.update(b".");
        h.update(label.as_bytes());
    }
}

/// Recompute the transaction message: domain preamble, then each preimage segment fed straight
/// into the hasher (mirrors the device, which streams chunks without buffering the whole preimage).
fn message(label: &str, segments: &[PreimageSegment]) -> [u8; 64] {
    let mut h = Blake2b512::new();
    add_domain_separation_tag(&mut h, TX_DOMAIN, TX_DOMAIN_VERSION, label);
    for segment in segments {
        h.update(&segment.bytes);
    }
    let mut out = [0u8; 64];
    out.copy_from_slice(&h.finalize());
    out
}

fn schnorr_challenge(r: &[u8; 32], p: &[u8; 32], m: &[u8; 64]) -> Scalar {
    let mut h = Blake2b512::new();
    add_domain_separation_tag(&mut h, SCHNORR_DOMAIN, SCHNORR_DOMAIN_VERSION, SCHNORR_LABEL);
    for chunk in [&r[..], &p[..], &m[..]] {
        h.update((chunk.len() as u64).to_le_bytes());
        h.update(chunk);
    }
    let mut out = [0u8; 64];
    out.copy_from_slice(&h.finalize());
    Scalar::from_bytes_mod_order_wide(&out)
}

fn deterministic_nonce(secret: &Scalar, m: &[u8; 64]) -> Scalar {
    let mut h = Blake2b512::new();
    h.update(NONCE_DOMAIN);
    h.update(secret.as_bytes());
    h.update(m);
    let mut out = [0u8; 64];
    out.copy_from_slice(&h.finalize());
    Scalar::from_bytes_mod_order_wide(&out)
}

fn public_key(s: &Scalar) -> [u8; 32] {
    (s * RISTRETTO_BASEPOINT_TABLE).compress().0
}

fn sign(secret: &Scalar, m: &[u8; 64]) -> ([u8; 32], [u8; 64]) {
    let pk = public_key(secret);
    let nonce = deterministic_nonce(secret, m);
    let r = public_key(&nonce);
    let e = schnorr_challenge(&r, &pk, m);
    let s = e * secret + nonce;
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&r);
    sig[32..].copy_from_slice(s.as_bytes());
    (pk, sig)
}

// ---- helpers ------------------------------------------------------------------------------------

fn random_scalar() -> Scalar {
    let mut b = [0u8; 64];
    rand::rng().fill_bytes(&mut b);
    Scalar::from_bytes_mod_order_wide(&b)
}

fn pk_bytes(s: &Scalar) -> RistrettoPublicKeyBytes {
    RistrettoPublicKeyBytes::from_bytes(&public_key(s)).unwrap()
}

fn sig_bytes(sig: &[u8; 64]) -> SchnorrSignatureBytes {
    SchnorrSignatureBytes::try_from(&sig[..]).unwrap()
}

fn sample_unsigned() -> UnsignedTransactionV1 {
    let mut blobs = Blobs::empty();
    blobs.push(Blob::from(vec![1u8, 2, 3])).unwrap();
    UnsignedTransactionV1 {
        network: 1,
        fee_instructions: vec![Instruction::DropAllProofsInWorkspace],
        instructions: vec![
            Instruction::DropAllProofsInWorkspace,
            Instruction::PutLastInstructionOutputOnWorkspace { key: 7 },
        ],
        inputs: IndexSet::new(),
        min_epoch: None,
        max_epoch: None,
        is_seal_signer_authorized: false,
        dry_run: true,
        blobs,
    }
}

// ---- tests --------------------------------------------------------------------------------------

#[test]
fn preimage_field_tags_match_protocol() {
    use ootle_ledger_common::arg_types::SigningField;
    use tari_ootle_transaction::PreimageField;

    // The host maps PreimageField -> SigningField by numeric tag, and the device validates field
    // order by the same tags. They must agree.
    let pairs = [
        (PreimageField::SchemaVersion, SigningField::SchemaVersion),
        (PreimageField::SealSigner, SigningField::SealSigner),
        (PreimageField::Network, SigningField::Network),
        (PreimageField::FeeInstructions, SigningField::FeeInstructions),
        (PreimageField::Instructions, SigningField::Instructions),
        (PreimageField::Inputs, SigningField::Inputs),
        (PreimageField::MinEpoch, SigningField::MinEpoch),
        (PreimageField::MaxEpoch, SigningField::MaxEpoch),
        (
            PreimageField::IsSealSignerAuthorized,
            SigningField::IsSealSignerAuthorized,
        ),
        (PreimageField::DryRun, SigningField::DryRun),
        (PreimageField::BlobHashes, SigningField::BlobHashes),
        (PreimageField::Signatures, SigningField::Signatures),
    ];
    for (preimage, wire) in pairs {
        assert_eq!(preimage as u8, wire as u8, "tag mismatch for {preimage:?}");
    }
}

#[test]
fn add_signer_recipe_matches_and_verifies() {
    let seal_signer = pk_bytes(&random_scalar());
    let signer = random_scalar();
    let tx = sample_unsigned();

    // (1) message recipe == create_message_v1
    let m = message(
        TX_LABEL_SIGNATURE,
        &TransactionSignature::signing_preimage_v1(&seal_signer, &tx),
    );
    assert_eq!(
        m,
        TransactionSignature::create_message_v1(&seal_signer, &tx),
        "message mismatch"
    );

    // (2) signature produced by the recipe verifies under tari_crypto
    let (pk, sig) = sign(&signer, &m);
    let signature = TransactionSignature::new(RistrettoPublicKeyBytes::from_bytes(&pk).unwrap(), sig_bytes(&sig));
    assert!(
        signature.verify_v1(&seal_signer, &tx),
        "authorization signature failed to verify"
    );
}

#[test]
fn seal_recipe_matches_and_verifies() {
    let unsigned = sample_unsigned();
    let seal_signer = random_scalar();
    let seal_signer_pk = pk_bytes(&seal_signer);

    // A prior authorization signature (its bytes are bound by the seal).
    let signer = random_scalar();
    let auth_msg = message(
        TX_LABEL_SIGNATURE,
        &TransactionSignature::signing_preimage_v1(&seal_signer_pk, &unsigned),
    );
    let (auth_pk, auth_sig) = sign(&signer, &auth_msg);
    let prior = TransactionSignature::new(
        RistrettoPublicKeyBytes::from_bytes(&auth_pk).unwrap(),
        sig_bytes(&auth_sig),
    );

    let unsealed = UnsealedTransactionV1::new(unsigned, vec![prior]);

    // (1) seal message recipe == create_message_v1
    let m = message(TX_LABEL_SEAL, &TransactionSealSignature::signing_preimage_v1(&unsealed));
    assert_eq!(
        m,
        TransactionSealSignature::create_message_v1(&unsealed),
        "seal message mismatch"
    );

    // (2) seal signature verifies
    let (pk, sig) = sign(&seal_signer, &m);
    let seal = TransactionSealSignature::new(RistrettoPublicKeyBytes::from_bytes(&pk).unwrap(), sig_bytes(&sig));
    assert!(seal.verify_v1(&unsealed), "seal signature failed to verify");
}
