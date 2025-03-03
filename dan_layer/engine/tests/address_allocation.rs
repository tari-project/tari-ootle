//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::Value;
use tari_bor::Value::Tag;
use tari_dan_engine::runtime::TransactionCommitError;
use tari_template_abi::Type;
use tari_template_lib::models::BinaryTag;
use tari_template_lib::{
    args,
    models::{ComponentAddress, ResourceAddress},
};
use tari_template_test_tooling::{support::assert_error::assert_reject_reason, TemplateTest};
use tari_transaction::Transaction;
use wasmer::IntoBytes;

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

    // assert execution results
    let comp_addr_allocation_exec_result = result.finalize.execution_results.first().unwrap().clone();
    let Type::Other {name: comp_alloc_return_type_name} = comp_addr_allocation_exec_result.return_type else { panic!("Invalid return type for component address allocation!") };
    assert_eq!(comp_alloc_return_type_name, "ComponentAddressAllocation".to_string());
    let exec_result_allocated_comp_addr: ComponentAddress = address_allocation_from_cbor(
        comp_addr_allocation_exec_result.indexed.clone().into_value()
    ).unwrap();

    let res_addr_allocation_exec_result = result.finalize.execution_results.get(1).unwrap().clone();
    let Type::Other {name: res_alloc_return_type_name} = res_addr_allocation_exec_result.return_type else { panic!("Invalid return type for resource address allocation!") };
    assert_eq!(res_alloc_return_type_name, "ResourceAddressAllocation".to_string());
    let exec_result_allocated_res_addr: ResourceAddress = address_allocation_from_cbor(
        res_addr_allocation_exec_result.indexed.clone().into_value()
    ).unwrap();

    let actual_comp = result
        .finalize
        .result
        .accept()
        .unwrap()
        .up_iter()
        .find_map(|(k, _)| k.as_component_address())
        .unwrap();

    let allocated_comp_after_func_call: ComponentAddress = address_from_cbor(
        result.finalize.execution_results[2].clone().indexed.into_value()
    ).unwrap();

    assert_eq!(actual_comp, allocated_comp_after_func_call);
    assert_eq!(actual_comp, exec_result_allocated_comp_addr);

    let (container_comp_id, container_comp_value) = result
        .finalize
        .result
        .accept()
        .unwrap()
        .up_iter()
        .filter(|(k, _)| k.as_component_address().is_some())
        .last()
        .unwrap();

    // TODO: add other checks (new components contains the right addresses etc..)
}

fn address_from_cbor<T: TryFrom<Vec<u8>>>(value: Value) -> Option<T> {
    if let Tag(_, address_raw) = value {
        if let Ok(address_bytes) = address_raw.into_bytes() {
            if let Ok(address) = address_bytes.try_into() {
                return Some(address);
            }
        }
    }
    None
}

fn address_allocation_from_cbor<T: TryFrom<Vec<u8>>>(value: Value) -> Option<T> {
    if let Tag(_, allocation_raw) = value {
        if let Ok(properties) = allocation_raw.into_map() {
            return properties.iter()
                .filter_map(|(key, value)| {
                    if let Some(property_id) = key.as_text() {
                        if property_id == "address" {
                            return address_from_cbor::<T>(value.clone());
                        }
                    }
                    None
                }).last();
        }
    }
    None
}
