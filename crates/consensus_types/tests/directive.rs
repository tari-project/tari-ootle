//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::rngs::OsRng;
use tari_consensus_types::{ConsensusDirective, DirectiveBody, DirectiveError, DirectiveKind};
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey};
use tari_ootle_common_types::Epoch;

fn sample_body() -> DirectiveBody {
    DirectiveBody {
        kind: DirectiveKind::rollback_to_epoch(Epoch(42)),
        nonce: 7,
        issued_at_unix_secs: 1_700_000_000,
    }
}

#[test]
fn sign_verify_roundtrip() {
    let (secret, public) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let directive = ConsensusDirective::sign(sample_body(), &secret, &mut OsRng).unwrap();

    // Correct public key verifies.
    directive.verify(&public).unwrap();
}

#[test]
fn verify_fails_with_wrong_public_key() {
    let (secret_a, _public_a) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let (_secret_b, public_b) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let directive = ConsensusDirective::sign(sample_body(), &secret_a, &mut OsRng).unwrap();

    match directive.verify(&public_b) {
        Err(DirectiveError::InvalidSignature) => (),
        other => panic!("expected InvalidSignature, got {:?}", other),
    }
}

#[test]
fn tampered_body_bytes_fail_verification() {
    let (secret, public) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let directive = ConsensusDirective::sign(sample_body(), &secret, &mut OsRng).unwrap();

    // Encode, flip a byte inside the body region (first byte encodes the DirectiveKind enum
    // discriminant — mutating the nonce offset is safer since there's only one kind variant today).
    // Body layout from borsh: kind (enum discriminant + payload: target_epoch u64 = 8B), nonce u64 (8B),
    // issued_at_unix_secs u64 (8B). Flip a bit in the nonce region.
    let mut bytes = borsh::to_vec(&directive).unwrap();
    // Discriminant (1B) + target_epoch (8B) = 9; first byte of nonce is at 9.
    bytes[9] ^= 0xFF;

    let tampered: ConsensusDirective = borsh::from_slice(&bytes).unwrap();
    match tampered.verify(&public) {
        Err(DirectiveError::InvalidSignature) => (),
        other => panic!("expected InvalidSignature after body tamper, got {:?}", other),
    }
}

#[test]
fn id_is_stable_across_serialisation() {
    let (secret, _public) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let directive = ConsensusDirective::sign(sample_body(), &secret, &mut OsRng).unwrap();
    let id_before = directive.id();

    let bytes = borsh::to_vec(&directive).unwrap();
    let decoded: ConsensusDirective = borsh::from_slice(&bytes).unwrap();
    let id_after = decoded.id();

    assert_eq!(id_before, id_after);
}

#[test]
fn same_body_same_id() {
    let (secret, _public) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let d1 = ConsensusDirective::sign(sample_body(), &secret, &mut OsRng).unwrap();
    let d2 = ConsensusDirective::sign(sample_body(), &secret, &mut OsRng).unwrap();
    assert_eq!(d1.id(), d2.id(), "ID is derived from body only, not the signature");
}

#[test]
fn different_nonce_different_id() {
    let (secret, _public) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let mut body_a = sample_body();
    let mut body_b = sample_body();
    body_a.nonce = 1;
    body_b.nonce = 2;
    let d_a = ConsensusDirective::sign(body_a, &secret, &mut OsRng).unwrap();
    let d_b = ConsensusDirective::sign(body_b, &secret, &mut OsRng).unwrap();
    assert_ne!(d_a.id(), d_b.id());
}
