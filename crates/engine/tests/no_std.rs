//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::indexed_value::IndexedValue;
use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::ComponentAddress;
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn it_can_call_a_method() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/no_std"]);

    let template = test.get_template_address("NoStdCounter");

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .allocate_component_address("no_std_counter")
            .call_function(template, "with_address", args![Workspace("no_std_counter")])
            .call_method("no_std_counter", "increment", args![])
            .build_and_seal(test.secret_key()),
        // Because we deny_all on deposits, we need to supply the owner proof to be able to deposit the initial
        // tokens into the new vaults
        vec![test.owner_proof()],
    );

    let component_address: ComponentAddress = result.finalize.execution_results[1].decode().unwrap();

    let component = test.read_only_state_store().get_component(component_address).unwrap();

    // NoStdCounter has a single `value: u64` field — minicbor encodes single-field structs as a
    // 1-element array indexed by `#[n(0)]`, so the value lives at `$.0`.
    let indexed = IndexedValue::from_value(component.into_state()).unwrap();
    let value: u64 = indexed.get_value("$.0").unwrap().unwrap();

    assert_eq!(value, 1);
}

#[test]
fn it_can_call_a_function() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/no_std"]);

    let template = test.get_template_address("NoStdCounter");

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_function(template, "simple", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );
}

#[test]
fn it_panics() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/no_std"]);

    let template = test.get_template_address("NoStdCounter");

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_function(template, "panic_works", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, "Panic works in no_std!");
}
