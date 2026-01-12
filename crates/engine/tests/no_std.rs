//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::indexed_value::IndexedValue;
use tari_template_lib::models::ComponentAddress;
use tari_template_test_tooling::TemplateTest;
use tari_transaction::{args, Transaction};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn it_compiles_when_using_no_std() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/no_std"]);

    let access_rules_template = test.get_template_address("NoStdCounter");

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .allocate_component_address("no_std_counter")
            .call_function(access_rules_template, "with_address", args![Workspace(
                "no_std_counter"
            )])
            .call_method("no_std_counter", "increment", args![])
            .build_and_seal(test.secret_key()),
        // Because we deny_all on deposits, we need to supply the owner proof to be able to deposit the initial
        // tokens into the new vaults
        vec![test.owner_proof()],
    );

    let component_address: ComponentAddress = result.finalize.execution_results[1].decode().unwrap();

    let component = test.read_only_state_store().get_component(component_address).unwrap();

    let value: u128 = IndexedValue::from_value(component.into_state())
        .unwrap()
        .get_value("$.value")
        .unwrap()
        .unwrap();

    assert_eq!(value, 1);
}
