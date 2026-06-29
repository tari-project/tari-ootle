//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! End-to-end engine proof of the publish-template stress flow: a transaction sealed by a fresh
//! random "author" key publishes a binary while the fee is authorised by a *different* fixed account
//! owner, and republishing the same binary under a new author does not collide on a duplicate
//! substate.
//!
//! This exercises the real auth path: the processor takes the published template's author from the
//! transaction's main signer (the random seal key) and `try_execute` derives the authorisation scope
//! from `signers_iter()` (which includes the fee owner's additional signature), mirroring the
//! validator-node executor.

use std::collections::HashMap;

use tari_engine_types::{
    commit_result::ExecuteResult,
    substate::{SubstateId, SubstateValue},
};
use tari_ootle_transaction::{Blob, Network};
use tari_template_test_tooling::{TemplateTest, compile::compile_template};
use tari_transaction_manifest::ManifestValue;
use transaction_generator::transaction_builders::manifest;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/manifests/publish_template.rs");

#[test]
fn random_signer_publishes_same_binary_twice_without_duplicating() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    test.enable_fees();

    // The fee-paying account and its owner key. The owner authorises pay_fee but is NOT the template
    // author.
    let (account_address, _owner_proof, account_key, _owner_public) = test.create_funded_account_with_keypair();

    // Reuse the max_compute stress template's WASM as the binary to publish.
    let compiled = compile_template("templates/max_compute", &[]).unwrap();
    let wasm = compiled.code().to_vec();

    let mut globals = HashMap::new();
    globals.insert(
        "account".to_string(),
        account_address.to_string().parse::<ManifestValue>().unwrap(),
    );
    let mut blob_inputs = HashMap::new();
    blob_inputs.insert("template".to_string(), Blob::from(wasm));

    let build = manifest::builder(
        account_key,
        Network::LocalNet,
        MANIFEST,
        globals,
        HashMap::new(),
        Vec::new(),
        blob_inputs,
        true,
    )
    .unwrap();

    // First publish: sealed by a random author, fee authorised by the account owner (auto-derived
    // from the transaction signers — pass no explicit proofs).
    let tx0 = build(0);
    let author0 = tx0.seal_signature().public_key().to_string();
    let result0 = test.execute_expect_success(tx0, vec![]);
    let (addr0, recorded_author0) = published_template(&result0);
    assert_eq!(
        recorded_author0, author0,
        "the template author must be the random seal key"
    );

    // Second publish of the SAME binary under a fresh author must succeed with a different address —
    // proving the per-transaction author keeps republishes from colliding on a duplicate substate.
    let tx1 = build(1);
    let author1 = tx1.seal_signature().public_key().to_string();
    assert_ne!(author0, author1, "each transaction must seal with a fresh author key");
    let result1 = test.execute_expect_success(tx1, vec![]);
    let (addr1, _recorded_author1) = published_template(&result1);

    assert_ne!(
        addr0, addr1,
        "republishing the same binary must yield a distinct template address"
    );
}

/// Extracts the (address, author) of the single template published in a result's accepted diff.
fn published_template(result: &ExecuteResult) -> (SubstateId, String) {
    result
        .expect_success()
        .up_iter()
        .find_map(|(id, substate)| match substate.substate_value() {
            SubstateValue::Template(t) if matches!(id, SubstateId::Template(_)) => {
                Some((id.clone(), t.author.to_string()))
            },
            _ => None,
        })
        .expect("a published template in the diff")
}
