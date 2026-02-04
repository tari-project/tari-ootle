//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine::runtime::TransactionCommitError;
use tari_engine_types::{component::derive_component_address_from_public_key, indexed_value::IndexedValue};
use tari_ootle_transaction::{Transaction, args};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::types::{ComponentAddress, ResourceAddress};
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason, xtr_faucet_component};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn it_allocates_addresses_in_template_code() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/address_allocation"]);

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .call_function(test.get_template_address("AddressAllocationTest"), "create", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let component_addr = result
        .finalize
        .result
        .any_accept()
        .unwrap()
        .up_iter()
        .find_map(|(k, _)| k.as_component_address())
        .unwrap();

    let allocated = result.finalize.execution_results[0]
        .indexed
        .decoded::<ComponentAddress>()
        .unwrap();
    assert_eq!(component_addr, allocated);

    let actual = result
        .finalize
        .result
        .any_accept()
        .unwrap()
        .up_iter()
        .find_map(|(k, _)| k.as_resource_address())
        .unwrap();

    let component_state = result
        .finalize
        .result
        .any_accept()
        .unwrap()
        .up_iter()
        .find_map(|(_, substate)| substate.substate_value().component())
        .unwrap();

    let component_state = IndexedValue::from_value(component_state.state().clone()).unwrap();

    let allocated = component_state
        .get_value::<ResourceAddress>("$.resource")
        .unwrap()
        .unwrap();
    assert_eq!(actual, allocated);
}

#[test]
fn it_fails_if_address_allocation_is_not_used() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/address_allocation"]);
    let template_addr = test.get_template_address("AddressAllocationTest");

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_function(template_addr, "drop_component_allocation", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, TransactionCommitError::DanglingAddressAllocations { count: 1 });
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_function(template_addr, "drop_resource_allocation", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, TransactionCommitError::DanglingAddressAllocations { count: 1 });
}

#[test]
fn it_fails_if_instruction_allocated_addresses_are_not_used() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/address_allocation"]);

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .allocate_component_address("my_addr")
            .allocate_resource_address("my_res")
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, TransactionCommitError::DanglingAddressAllocations { count: 2 });
}

#[test]
fn it_allocates_an_address_using_instructions() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/address_allocation"]);

    let template_addr = test.get_template_address("AddressAllocationTest");

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .allocate_component_address("my_addr")
            .allocate_resource_address("my_res")
            .call_function(template_addr, "get_component_allocation_address", args![Workspace(
                "my_addr"
            )])
            .call_function(template_addr, "get_resource_allocation_address", args![Workspace(
                "my_res"
            )])
            .call_function(template_addr, "create_from_allocations", args![
                Workspace("my_addr"),
                Workspace("my_res")
            ])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let actual_comp = result
        .finalize
        .result
        .any_accept()
        .unwrap()
        .up_iter()
        .find_map(|(k, _)| k.as_component_address())
        .unwrap();

    let addr = result.expect_return::<String>(2);
    assert_eq!(addr, actual_comp.to_string());

    let actual_resx = result
        .finalize
        .result
        .any_accept()
        .unwrap()
        .up_iter()
        .find_map(|(k, _)| k.as_resource_address())
        .unwrap();

    let addr = result.expect_return::<String>(3);
    assert_eq!(addr, actual_resx.to_string());
}

#[test]
fn it_allows_calls_to_component_using_the_allocated_address() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/address_allocation"]);

    let template_addr = test.get_template_address("AddressAllocationTest");

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .allocate_component_address("my_addr")
            .allocate_resource_address("my_res")
            .call_function(template_addr, "create_from_allocations", args![
                Workspace("my_addr"),
                Workspace("my_res")
            ])
            .call_method("my_addr", "get_resource_address", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let actual_resx = result
        .expect_success()
        .up_iter()
        .find_map(|(k, _)| k.as_resource_address())
        .unwrap();

    let addr = result.expect_return::<ResourceAddress>(3);
    assert_eq!(addr, actual_resx);
}

#[test]
fn it_allows_calls_to_component_using_a_component_on_the_workspace() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/address_allocation"]);

    let expected_account_addr =
        derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &test.to_public_key_bytes());

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(xtr_faucet_component(), "take", args![1000])
            .put_last_instruction_output_on_workspace("bucket")
            .create_account(test.to_public_key_bytes())
            // Put the created account address on the workspace, and
            .put_last_instruction_output_on_workspace("account")
            // use it to call deposit
            .call_method("account", "deposit", args![Workspace("bucket")])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let _account = test.read_only_state_store().get_account(expected_account_addr).unwrap();
}
