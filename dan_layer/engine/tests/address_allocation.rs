//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::Value;
use tari_bor::Value::Tag;
use tari_dan_engine::runtime::TransactionCommitError;
use tari_template_lib::models::BinaryTag;
use tari_template_lib::{
    args,
    models::{ComponentAddress, ResourceAddress},
};
use tari_template_test_tooling::{support::assert_error::assert_reject_reason, TemplateTest};
use tari_transaction::Transaction;

const ALLOCATED_COMPONENT_ADDRESS_TAG: u64 = BinaryTag::AllocatedComponentAddress.as_u64();
const ALLOCATED_RESOURCE_ADDRESS_TAG: u64 = BinaryTag::AllocatedResourceAddress.as_u64();

#[test]
fn it_uses_allocate_component_address() {
    let mut test = TemplateTest::new(["tests/templates/component_address_allocation"]);

    let result = test.execute_expect_success(
        Transaction::builder()
            .call_function(test.get_template_address("AddressAllocationTest"), "create", args![])
            .build_and_seal(test.get_test_secret_key()),
        vec![],
    );

    let actual = result
        .finalize
        .result
        .accept()
        .unwrap()
        .up_iter()
        .find_map(|(k, _)| k.as_component_address())
        .unwrap();

    let allocated = result.finalize.execution_results[0]
        .indexed
        .get_value::<ComponentAddress>("$.1")
        .unwrap()
        .unwrap();
    assert_eq!(actual, allocated);
}

#[test]
fn it_fails_if_component_address_allocation_is_not_used() {
    let mut test = TemplateTest::new(["tests/templates/component_address_allocation"]);
    let template_addr = test.get_template_address("AddressAllocationTest");

    let reason = test.execute_expect_failure(
        Transaction::builder()
            .call_function(template_addr, "drop_allocation", args![])
            .build_and_seal(test.get_test_secret_key()),
        vec![],
    );

    assert_reject_reason(reason, TransactionCommitError::DanglingAddressAllocations { count: 1 });
}

#[test]
fn it_uses_allocate_resource_address() {
    let mut test = TemplateTest::new(["tests/templates/resource_address_allocation"]);

    let result = test.execute_expect_success(
        Transaction::builder()
            .call_function(
                test.get_template_address("ResourceAddressAllocationTest"),
                "create",
                args![],
            )
            .build_and_seal(test.get_test_secret_key()),
        vec![],
    );

    let actual = result
        .finalize
        .result
        .accept()
        .unwrap()
        .up_iter()
        .find_map(|(k, _)| k.as_resource_address())
        .unwrap();

    let allocated = result.finalize.execution_results[0]
        .indexed
        .get_value::<ResourceAddress>("$.1")
        .unwrap()
        .unwrap();
    assert_eq!(actual, allocated);
}

#[test]
fn it_fails_if_resource_address_allocation_is_not_used() {
    let mut test = TemplateTest::new(["tests/templates/resource_address_allocation"]);
    let template_addr = test.get_template_address("ResourceAddressAllocationTest");

    let reason = test.execute_expect_failure(
        Transaction::builder()
            .call_function(template_addr, "drop_allocation", args![])
            .build_and_seal(test.get_test_secret_key()),
        vec![],
    );

    assert_reject_reason(reason, TransactionCommitError::DanglingAddressAllocations { count: 1 });
}

#[test]
fn it_allocation_using_tx_builder() {
    let mut test = TemplateTest::new([
        "tests/templates/address_allocation_from_instructions",
        "tests/templates/component_address_allocation_from_already_allocated",
    ]);

    let result = test.execute_expect_success(
        Transaction::builder()
            .allocate_address(args::SubstateType::Component, "my_addr")
            .allocate_address(args::SubstateType::Resource, "my_res")
            .call_function(test.get_template_address("ComponentAddressAllocationTest"), "create", args![Workspace("my_addr")])
            .call_function(
                test.get_template_address("AddressAllocationFromInstructionsTest"),
                "create",
                args![Workspace("my_addr"), Workspace("my_res")],
            )
            .build_and_seal(test.get_test_secret_key()),
        vec![],
    );

    let actual = result
        .finalize
        .result
        .accept()
        .unwrap()
        .up_iter()
        .find_map(|(k, _)| k.as_component_address())
        .unwrap();
    
    let allocated = component_address_from_cbor(
        result.finalize.execution_results[2].clone().indexed.into_value()
    ).unwrap();
    
    // TODO: in transaction processor return the allocated address as a result, so it can be checked in the execution results
    // TODO: add other checks (new components contains the right addresses etc..)
    
    assert_eq!(actual, allocated);
}

fn component_address_from_cbor(value: Value) -> Option<ComponentAddress> {
    if let Tag(_, address_raw) = value {
        if let Ok(address_bytes) = address_raw.into_bytes() {
            if let Ok(address) = ComponentAddress::try_from(address_bytes.as_slice()) {
                return Some(address);
            }
        }
    }
    None
}
