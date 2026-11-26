//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Host reference for the on-device signing recipe (`ootle-ledger-app/src/hashing.rs`).
//!
//! This independently reimplements, using the `blake2`, `merlin` and `curve25519-dalek` crates, the
//! exact byte recipe the Ledger device follows to (1) recompute a transaction's 64-byte signing
//! message from the streamed preimage segments and (2) produce an sr25519 signature. It then proves
//! the result against `tari_ootle_transaction`:
//!   - the reference message equals `create_message` (domain separation + borsh feeding), and
//!   - the reference signature verifies via `verify_message` (sr25519 challenge + `s = e*k + r`).
//!
//! If this passes, the device recipe is correct by construction; only the SDK's Blake2b and the
//! APDU transport remain to be exercised on Speculos.

use blake2::{Blake2b512, Digest};
use curve25519_dalek::{Scalar, constants::RISTRETTO_BASEPOINT_TABLE, ristretto::CompressedRistretto};
use indexmap::IndexSet;
use ootle_ledger_common::signing::{
    NONCE_DOMAIN,
    SIGNING_CONTEXT,
    STEALTH_OWNER_LABEL,
    TX_DOMAIN,
    TX_DOMAIN_VERSION,
    TX_LABEL_SEAL,
    TX_LABEL_SIGNATURE,
    WALLET_DOMAIN,
    WALLET_DOMAIN_VERSION,
};
use rand::Rng;
use tari_ootle_common_types::signature::Sr25519SignatureBytes;
use tari_ootle_transaction::{
    AuthorizationContext,
    AuthorizerSet,
    Blob,
    Blobs,
    Epoch,
    Instruction,
    MainSigner,
    PreimageSegment,
    TransactionSealSignature,
    TransactionSignature,
    UnsealedTransactionV1,
    UnsignedTransactionV1,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

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
    let mut transcript = merlin::Transcript::new(b"SigningContext");
    transcript.append_message(b"", SIGNING_CONTEXT);
    transcript.append_message(b"sign-bytes", m);
    transcript.append_message(b"proto-name", b"Schnorr-sig");
    transcript.append_message(b"sign:pk", p);
    transcript.append_message(b"sign:R", r);
    let mut out = [0u8; 64];
    transcript.challenge_bytes(b"sign:c", &mut out);
    Scalar::from_bytes_mod_order_wide(&out)
}

fn deterministic_nonce(secret: &Scalar, pk: &[u8; 32], m: &[u8; 64]) -> Scalar {
    let mut h = Blake2b512::new();
    h.update(NONCE_DOMAIN);
    h.update((SIGNING_CONTEXT.len() as u64).to_le_bytes());
    h.update(SIGNING_CONTEXT);
    h.update(pk);
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
    let nonce = deterministic_nonce(secret, &pk, m);
    let r = public_key(&nonce);
    let e = schnorr_challenge(&r, &pk, m);
    let s = e * secret + nonce;
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&r);
    sig[32..].copy_from_slice(s.as_bytes());
    sig[63] |= 0x80;
    (pk, sig)
}

/// The stealth signing-key tweak applied before signing a confidential transfer (mirrors
/// `derive_stealth_secret` in `src/hashing.rs`): `c + k` where `c = H_stealth_owner(network, v·R)`,
/// `v` is the account's view secret and `R` is the spent UTXO's sender public nonce. The DH point is
/// compressed and fed borsh-encoded (`u32_le(len) || bytes`), matching
/// `DomainSeparatedBorshHasher::update_consensus_encode` on a byte slice in
/// `tari_ootle_wallet_crypto::kdfs::dh_kdf_aead`.
fn derive_stealth_secret(
    network_byte: u8,
    account_secret: &Scalar,
    view_secret: &Scalar,
    public_nonce: &[u8; 32],
) -> Scalar {
    let r = CompressedRistretto(*public_nonce)
        .decompress()
        .expect("public nonce is a valid Ristretto point");
    let dh = (view_secret * r).compress().0;

    let label = format!("{STEALTH_OWNER_LABEL}.n{network_byte}");
    let mut h = Blake2b512::new();
    add_domain_separation_tag(&mut h, WALLET_DOMAIN, WALLET_DOMAIN_VERSION, &label);
    h.update((dh.len() as u32).to_le_bytes());
    h.update(dh);
    let mut out = [0u8; 64];
    out.copy_from_slice(&h.finalize());
    let c = Scalar::from_bytes_mod_order_wide(&out);
    c + account_secret
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

fn sig_bytes(sig: &[u8; 64]) -> Sr25519SignatureBytes {
    Sr25519SignatureBytes::from_array(*sig)
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
        max_epoch: Epoch(1),
        main_signer: MainSigner::None,
        dry_run: true,
        blobs,
        nonce: 42,
    }
}

// ---- tests --------------------------------------------------------------------------------------

#[test]
fn signing_context_matches_protocol() {
    assert_eq!(
        SIGNING_CONTEXT,
        tari_ootle_transaction::TRANSACTION_SIGNING_CONTEXT.as_bytes()
    );
    assert_eq!(
        ootle_ledger_common::arg_types::EXPECTED_SCHEMA_VERSION,
        tari_ootle_transaction::SchemaVersion::CURRENT.as_u16()
    );
}

#[test]
fn preimage_field_tags_match_protocol() {
    use ootle_ledger_common::arg_types::SigningField;
    use tari_ootle_transaction::PreimageField;

    // The host maps PreimageField -> SigningField by numeric tag, and the device checks each
    // segment's tag against its expected sequence. They must agree.
    let pairs = [
        (PreimageField::SchemaVersion, SigningField::SchemaVersion),
        (PreimageField::SealSigner, SigningField::SealSigner),
        (PreimageField::Network, SigningField::Network),
        (PreimageField::FeeInstructions, SigningField::FeeInstructions),
        (PreimageField::Instructions, SigningField::Instructions),
        (PreimageField::Inputs, SigningField::Inputs),
        (PreimageField::MinEpoch, SigningField::MinEpoch),
        (PreimageField::MaxEpoch, SigningField::MaxEpoch),
        (PreimageField::MainSigner, SigningField::MainSigner),
        (PreimageField::DryRun, SigningField::DryRun),
        (PreimageField::Nonce, SigningField::TransactionNonce),
        (PreimageField::BlobHashes, SigningField::BlobHashes),
        (PreimageField::Signatures, SigningField::Signatures),
        (PreimageField::Authorizers, SigningField::Authorizers),
    ];
    for (preimage, wire) in pairs {
        assert_eq!(preimage as u8, wire as u8, "tag mismatch for {preimage:?}");
    }
}

#[test]
fn expected_field_sequences_match_preimage_v1() {
    use ootle_ledger_common::arg_types::{SignMode, SigningField};

    let seal_signer = pk_bytes(&random_scalar());
    let tx = sample_unsigned();
    let authorizers: AuthorizerSet = [pk_bytes(&random_scalar())].into_iter().collect();
    let add_signer_segments =
        TransactionSignature::signing_preimage(AuthorizationContext::new(&seal_signer, &authorizers), &tx);
    let add_signer_fields: Vec<SigningField> = add_signer_segments
        .iter()
        .map(|s| SigningField::try_from(s.field as u8).unwrap())
        .collect();
    assert_eq!(add_signer_fields, SignMode::AddSigner.expected_fields());

    let unsealed = UnsealedTransactionV1::new(tx, vec![]);
    let seal_segments = TransactionSealSignature::signing_preimage(&unsealed);
    let seal_fields: Vec<SigningField> = seal_segments
        .iter()
        .map(|s| SigningField::try_from(s.field as u8).unwrap())
        .collect();
    assert_eq!(seal_fields, SignMode::Seal.expected_fields());
}

#[test]
fn add_signer_recipe_matches_and_verifies() {
    let seal_signer = pk_bytes(&random_scalar());
    let signer = random_scalar();
    let tx = sample_unsigned();
    let authorizers: AuthorizerSet = [pk_bytes(&signer), pk_bytes(&random_scalar())].into_iter().collect();
    let context = AuthorizationContext::new(&seal_signer, &authorizers);

    // (1) message recipe == create_message
    let m = message(
        TX_LABEL_SIGNATURE,
        &TransactionSignature::signing_preimage(context, &tx),
    );
    assert_eq!(
        &m,
        TransactionSignature::create_message(context, &tx).as_bytes(),
        "message mismatch"
    );

    // (2) signature produced by the recipe verifies under tari_crypto
    let (pk, sig) = sign(&signer, &m);
    let signature = TransactionSignature::new(RistrettoPublicKeyBytes::from_bytes(&pk).unwrap(), sig_bytes(&sig));
    assert!(
        signature.verify_message(TransactionSignature::create_message(context, &tx)),
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
    let authorizers: AuthorizerSet = [pk_bytes(&signer)].into_iter().collect();
    let auth_msg = message(
        TX_LABEL_SIGNATURE,
        &TransactionSignature::signing_preimage(AuthorizationContext::new(&seal_signer_pk, &authorizers), &unsigned),
    );
    let (auth_pk, auth_sig) = sign(&signer, &auth_msg);
    let prior = TransactionSignature::new(
        RistrettoPublicKeyBytes::from_bytes(&auth_pk).unwrap(),
        sig_bytes(&auth_sig),
    );

    let unsealed = UnsealedTransactionV1::new(unsigned, vec![prior]);

    // (1) seal message recipe == create_message
    let m = message(TX_LABEL_SEAL, &TransactionSealSignature::signing_preimage(&unsealed));
    assert_eq!(
        &m,
        TransactionSealSignature::create_message(&unsealed).as_bytes(),
        "seal message mismatch"
    );

    // (2) seal signature verifies
    let (pk, sig) = sign(&seal_signer, &m);
    let seal = TransactionSealSignature::new(RistrettoPublicKeyBytes::from_bytes(&pk).unwrap(), sig_bytes(&sig));
    assert!(
        seal.verify_message(TransactionSealSignature::create_message(&unsealed)),
        "seal signature failed to verify"
    );
}

#[test]
fn stealth_recipe_matches_and_verifies() {
    use ootle_ledger_common::arg_types::{GetPublicKeyRequest, KeyType, StealthTweak};
    use ootle_network::Network;
    use tari_crypto::{
        keys::{PublicKey, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
        tari_utilities::ByteArray,
    };
    use tari_ootle_wallet_crypto::kdfs::{owner_stealth_dh_secret, owner_stealth_dh_stealth_address};

    let network = Network::LocalNet;
    let mut rng = rand::rng();
    // Account spend key k, view key v, and the sender ephemeral nonce (r, R) carried by the spent UTXO.
    let k = RistrettoSecretKey::random(&mut rng);
    let k_pub = RistrettoPublicKey::from_secret_key(&k);
    let v = RistrettoSecretKey::random(&mut rng);
    let v_pub = RistrettoPublicKey::from_secret_key(&v);
    let r = RistrettoSecretKey::random(&mut rng);
    let r_pub = RistrettoPublicKey::from_secret_key(&r);

    // Device inputs: the account and view scalars and the compressed public nonce R.
    let k_scalar = Scalar::from_bytes_mod_order(<[u8; 32]>::try_from(k.as_bytes()).unwrap());
    let v_scalar = Scalar::from_bytes_mod_order(<[u8; 32]>::try_from(v.as_bytes()).unwrap());
    let r_bytes = <[u8; 32]>::try_from(r_pub.as_bytes()).unwrap();

    // (1) device tweak == owner_stealth_dh_secret
    let got = derive_stealth_secret(network.as_byte(), &k_scalar, &v_scalar, &r_bytes);
    let expected = owner_stealth_dh_secret(network, &k, &v, &r_pub);
    assert_eq!(&got.as_bytes()[..], expected.as_bytes(), "stealth secret mismatch");

    // (2) returned public key == the sender-derived stealth address C = H(r·V)·G + k·G
    let stealth_address = owner_stealth_dh_stealth_address(network, &k_pub, &v_pub, &r);
    assert_eq!(
        public_key(&got),
        <[u8; 32]>::try_from(stealth_address.as_bytes()).unwrap(),
        "stealth address mismatch"
    );

    // (3) an authorization signature produced with the stealth key verifies under tari_crypto
    let seal_signer = pk_bytes(&random_scalar());
    let tx = sample_unsigned();
    let authorizers: AuthorizerSet = [RistrettoPublicKeyBytes::from_bytes(&public_key(&got)).unwrap()]
        .into_iter()
        .collect();
    let context = AuthorizationContext::new(&seal_signer, &authorizers);
    let m = message(
        TX_LABEL_SIGNATURE,
        &TransactionSignature::signing_preimage(context, &tx),
    );
    let (pk, sig) = sign(&got, &m);
    let signature = TransactionSignature::new(RistrettoPublicKeyBytes::from_bytes(&pk).unwrap(), sig_bytes(&sig));
    assert!(
        signature.verify_message(TransactionSignature::create_message(context, &tx)),
        "stealth authorization signature failed to verify"
    );

    // (4) The tweak a `GetPublicKey` request carries must survive the wire and, fed to the same
    // derivation the signing path uses, reproduce the stealth address — that equality is what lets
    // a caller resolve the seal key before any signature binds to it. (Whether the device handler
    // applies the tweak at all is only observable on Speculos.)
    let request = GetPublicKeyRequest {
        account: 0,
        index: 0,
        key_type: KeyType::Account,
        stealth: Some(StealthTweak {
            network: network.as_byte(),
            public_nonce: r_bytes,
        }),
    };
    let decoded: GetPublicKeyRequest = borsh::from_slice(&borsh::to_vec(&request).unwrap()).unwrap();
    let tweak = decoded.stealth.expect("stealth tweak survives the wire");
    let returned = public_key(&derive_stealth_secret(
        tweak.network,
        &k_scalar,
        &v_scalar,
        &tweak.public_nonce,
    ));
    assert_eq!(
        returned,
        <[u8; 32]>::try_from(stealth_address.as_bytes()).unwrap(),
        "GetPublicKey stealth key mismatch"
    );

    // (5) The tweak binds both of its parts: the same nonce on another network, and another nonce
    // on the same network, must each yield a different key.
    let other_network = Network::Igor;
    assert_ne!(
        returned,
        public_key(&derive_stealth_secret(
            other_network.as_byte(),
            &k_scalar,
            &v_scalar,
            &r_bytes
        )),
        "stealth key is not bound to the network"
    );
    let other_nonce =
        <[u8; 32]>::try_from(RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::random(&mut rng)).as_bytes())
            .unwrap();
    assert_ne!(
        returned,
        public_key(&derive_stealth_secret(
            network.as_byte(),
            &k_scalar,
            &v_scalar,
            &other_nonce
        )),
        "stealth key is not bound to the public nonce"
    );
}

/// Apps up to 0.3 sign Tari Schnorr signatures over the same 64-byte messages with the nonce
/// `H("com.tari.ootle.ledger.schnorr_nonce.v1" || k || m)`. A nonce shared with that app hashes a
/// different challenge, so one signature from each over the same message solves for `k`.
#[test]
fn nonce_differs_from_the_tari_schnorr_app() {
    let secret = random_scalar();
    let pk = public_key(&secret);
    let m = *TransactionSignature::create_message(
        AuthorizationContext::new(&pk_bytes(&random_scalar()), &AuthorizerSet::new()),
        &sample_unsigned(),
    )
    .as_bytes();

    let mut h = Blake2b512::new();
    h.update(b"com.tari.ootle.ledger.schnorr_nonce.v1");
    h.update(secret.as_bytes());
    h.update(m);
    let mut out = [0u8; 64];
    out.copy_from_slice(&h.finalize());
    let tari_schnorr_nonce = Scalar::from_bytes_mod_order_wide(&out);

    assert_ne!(deterministic_nonce(&secret, &pk, &m), tari_schnorr_nonce);
}

#[test]
fn sign_stream_state_accepts_canonical_preimages_and_rejects_boundary_spoofing() {
    use ootle_ledger_common::{
        OotleStatusWord,
        arg_types::{SEGMENT_LAST_CHUNK, SignMode, SignStreamState, SigningField, TxDisplay},
    };
    use tari_ootle_common_types::InputDeclaration;
    use tari_template_lib_types::ComponentAddress;

    fn run_stream(
        mode: SignMode,
        segments: &[(SigningField, Vec<u8>)],
        max_chunk: usize,
    ) -> Result<TxDisplay, OotleStatusWord> {
        let mut stream = SignStreamState::new(mode);
        for (field, bytes) in segments {
            let tag = *field as u8;
            if bytes.is_empty() {
                stream.accept_segment(tag | SEGMENT_LAST_CHUNK, &[])?;
                continue;
            }
            let mut chunks = bytes.chunks(max_chunk).peekable();
            while let Some(chunk) = chunks.next() {
                let p1 = if chunks.peek().is_none() {
                    tag | SEGMENT_LAST_CHUNK
                } else {
                    tag
                };
                stream.accept_segment(p1, chunk)?;
            }
        }
        stream.finish()
    }

    let seal_signer = pk_bytes(&random_scalar());
    let authorizer = pk_bytes(&random_scalar());
    let mut tx = sample_unsigned();
    tx.min_epoch = Some(Epoch(7));
    tx.max_epoch = Epoch(99);
    tx.main_signer = MainSigner::Authorizer(authorizer);
    tx.inputs
        .insert(InputDeclaration::read(ComponentAddress::from_array([0x11; 32])));
    for i in 0..8u8 {
        tx.blobs.push(Blob::from(vec![i; 16])).unwrap();
    }

    let to_pairs = |segments: Vec<PreimageSegment>| -> Vec<(SigningField, Vec<u8>)> {
        segments
            .into_iter()
            .map(|s| (SigningField::try_from(s.field as u8).unwrap(), s.bytes))
            .collect()
    };

    let authorizers: AuthorizerSet = [authorizer].into_iter().collect();
    let context = AuthorizationContext::new(&seal_signer, &authorizers);
    let add_signer_segments = to_pairs(TransactionSignature::signing_preimage(context, &tx));
    let expected_display = TxDisplay {
        network: tx.network,
        fee_instruction_count: tx.fee_instructions.len() as u32,
    };
    assert_eq!(
        run_stream(SignMode::AddSigner, &add_signer_segments, 250),
        Ok(expected_display)
    );
    assert_eq!(
        run_stream(SignMode::AddSigner, &add_signer_segments, 64),
        Ok(expected_display)
    );

    let signer = random_scalar();
    let auth_msg = message(
        TX_LABEL_SIGNATURE,
        &TransactionSignature::signing_preimage(context, &tx),
    );
    let (auth_pk, auth_sig) = sign(&signer, &auth_msg);
    let prior = TransactionSignature::new(
        RistrettoPublicKeyBytes::from_bytes(&auth_pk).unwrap(),
        sig_bytes(&auth_sig),
    );
    let unsealed = UnsealedTransactionV1::new(tx.clone(), vec![prior.clone(), prior.clone(), prior]);
    let seal_segments = to_pairs(TransactionSealSignature::signing_preimage(&unsealed));
    assert_eq!(run_stream(SignMode::Seal, &seal_segments, 250), Ok(expected_display));
    assert_eq!(run_stream(SignMode::Seal, &seal_segments, 64), Ok(expected_display));

    // Shifting the network byte into SchemaVersion or appending bytes to SealSigner is rejected.
    let mut shifted_network = seal_segments.clone();
    shifted_network
        .iter_mut()
        .find(|(field, _)| *field == SigningField::SchemaVersion)
        .unwrap()
        .1
        .push(0x00);
    assert_eq!(
        run_stream(SignMode::Seal, &shifted_network, 250),
        Err(OotleStatusWord::BadRequest)
    );

    let mut shifted_seal_signer = add_signer_segments.clone();
    shifted_seal_signer
        .iter_mut()
        .find(|(field, _)| *field == SigningField::SealSigner)
        .unwrap()
        .1
        .push(0x00);
    assert_eq!(
        run_stream(SignMode::AddSigner, &shifted_seal_signer, 250),
        Err(OotleStatusWord::BadRequest)
    );

    // Shifting bytes into Network to spoof display.fee_instruction_count is rejected.
    let mut shifted_fee_count = seal_segments;
    shifted_fee_count
        .iter_mut()
        .find(|(field, _)| *field == SigningField::Network)
        .unwrap()
        .1
        .extend_from_slice(&[0u8, 0, 0, 0]);
    assert_eq!(
        run_stream(SignMode::Seal, &shifted_fee_count, 250),
        Err(OotleStatusWord::BadRequest)
    );
}
