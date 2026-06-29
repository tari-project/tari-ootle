//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Guards the publish-template stress flow (manifests/publish_template.rs + the `--random-signer`
//! generator mode): publishing one binary many times must produce a unique template address per
//! transaction while the fee is authorised by a single fixed account owner.

use std::collections::{HashMap, HashSet};

use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_ootle_transaction::{Blob, Instruction, Network};
use tari_transaction_manifest::ManifestValue;
use transaction_generator::transaction_builders::manifest;

const MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/manifests/publish_template.rs");
const ACCOUNT: &str = "component_f29021e8945394114b97c4b56c41fdf62a28e35fb8b6c1bff1c574f68ae28a54";

fn globals() -> HashMap<String, ManifestValue> {
    let mut g = HashMap::new();
    g.insert("account".to_string(), ACCOUNT.parse().unwrap());
    g
}

fn blob_inputs() -> HashMap<String, Blob> {
    // The manifest never validates the binary (that happens at execution); any bytes exercise the
    // blob-attachment and address-derivation paths.
    let mut b = HashMap::new();
    b.insert("template".to_string(), Blob::from(b"\0asm\x01\0\0\0dummy".to_vec()));
    b
}

#[test]
fn random_signer_makes_each_publish_address_distinct() {
    let owner_secret = RistrettoSecretKey::random(&mut rand::rng());
    let owner_public = RistrettoPublicKey::from_secret_key(&owner_secret);

    let build = manifest::builder(
        owner_secret,
        Network::LocalNet,
        MANIFEST,
        globals(),
        HashMap::new(),
        Vec::new(),
        blob_inputs(),
        true,
    )
    .unwrap();

    let mut seal_signers = HashSet::new();
    let mut template_addresses = HashSet::new();

    for i in 0..5 {
        let tx = build(i);

        assert!(tx.verify_all_signatures(), "all signatures must verify");

        // The fresh seal key is the authorised main signer, which the engine records as the template
        // author — that is what makes each published address unique.
        assert!(tx.is_seal_signer_authorized());
        let author = tx.seal_signature().public_key().to_string();
        assert!(
            seal_signers.insert(author),
            "each transaction must seal with a fresh author key"
        );

        // Exactly one additional signer: the fixed fee-paying account owner.
        let extra = tx.signatures();
        assert_eq!(extra.len(), 1, "expected exactly the fee-owner signature");
        assert_eq!(
            *extra[0].public_key(),
            ootle_byte_type::ToByteType::to_byte_type(&owner_public),
            "the additional signer must be the fee-paying account owner",
        );

        // The transaction publishes exactly one template.
        let publishes = tx
            .instructions()
            .iter()
            .chain(tx.fee_instructions())
            .filter(|i| matches!(i, Instruction::PublishTemplate { .. }))
            .count();
        assert_eq!(publishes, 1, "expected a single PublishTemplate instruction");

        // The derived on-chain address (H(author_pk, binary_hash)) must be unique per transaction.
        let (addr, _bytes) = tx
            .all_published_templates_iter()
            .next()
            .expect("a published template address");
        assert!(
            template_addresses.insert(addr.to_string()),
            "republishing the same binary must yield a distinct template address",
        );
    }

    assert_eq!(seal_signers.len(), 5);
    assert_eq!(template_addresses.len(), 5);
}

#[test]
fn without_random_signer_a_single_signer_seals() {
    let signer = RistrettoSecretKey::random(&mut rand::rng());
    let signer_public = RistrettoPublicKey::from_secret_key(&signer);

    let build = manifest::builder(
        signer,
        Network::LocalNet,
        MANIFEST,
        globals(),
        HashMap::new(),
        Vec::new(),
        blob_inputs(),
        false,
    )
    .unwrap();

    let tx = build(0);
    assert!(tx.verify_all_signatures());
    // With no `--random-signer`, the lone signer seals and is authorised; there are no extra signers.
    assert!(tx.is_seal_signer_authorized());
    assert!(tx.signatures().is_empty());
    assert_eq!(
        *tx.seal_signature().public_key(),
        ootle_byte_type::ToByteType::to_byte_type(&signer_public),
    );
}
