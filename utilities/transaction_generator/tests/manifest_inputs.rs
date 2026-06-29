//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The manifest transaction builder must declare every substate the transaction touches as an
//! input, or the executor fails with SubstateNotFound. Substate-typed `--arg`s (e.g. the fee
//! account, referenced in the manifest) are auto-declared; `--input`s (e.g. a fee vault the manifest
//! debits but never names) are declared explicitly. This guards both paths via the max_compute
//! manifest.

use std::collections::HashMap;

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_ootle_common_types::SubstateRequirement;
use tari_ootle_transaction::Network;
use tari_template_lib_types::TemplateAddress;
use tari_transaction_manifest::ManifestValue;
use transaction_generator::transaction_builders::manifest;

const MANIFEST: &str = "manifests/max_compute.rs";
const ACCOUNT: &str = "component_1111111111111111111111111111111111111111111111111111111111111111";
const VAULT: &str = "vault_2222222222222222222222222222222222222222222222222222222222222222";
const TEMPLATE: &str = "3333333333333333333333333333333333333333333333333333333333333333";

#[test]
fn declares_account_arg_and_explicit_input_as_transaction_inputs() {
    let globals = HashMap::from([("account".to_string(), ACCOUNT.parse::<ManifestValue>().unwrap())]);
    let templates = HashMap::from([("MaxCompute".to_string(), TemplateAddress::from_hex(TEMPLATE).unwrap())]);
    let extra_inputs = vec![VAULT.parse::<SubstateRequirement>().unwrap()];

    let build = manifest::builder(
        RistrettoSecretKey::default(),
        Network::LocalNet,
        MANIFEST,
        globals,
        templates,
        extra_inputs,
        HashMap::new(),
        false,
    )
    .unwrap();

    let transaction = build(0);
    let inputs: Vec<String> = transaction
        .inputs()
        .iter()
        .map(|r| r.substate_id().to_string())
        .collect();

    assert!(
        inputs.iter().any(|id| id == ACCOUNT),
        "account arg should be declared as an input, got {inputs:?}",
    );
    assert!(
        inputs.iter().any(|id| id == VAULT),
        "explicit --input vault should be declared as an input, got {inputs:?}",
    );
}
