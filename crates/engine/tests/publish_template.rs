// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::iter;

use ootle_byte_type::ToByteType;
use rand::random;
use tari_engine::transaction::TransactionErrorKind;
use tari_engine_types::{
    commit_result::{ExecutionFailureCode, RejectReason, TransactionResult},
    hashing::hash_template_code,
    limits,
    published_template::PublishedTemplateAddress,
    substate::{SubstateId, SubstateValue},
};
use tari_ootle_transaction::{Epoch, Transaction};
use tari_template_abi::TEMPLATE_DEF_CUSTOM_SECTION;
use tari_template_test_tooling::{
    TemplateTest,
    compile::compile_template,
    support::assert_error::assert_reject_reason,
};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn publish_template_success() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    let (account_address, owner_proof, account_key, public_key) = test.create_funded_account_with_keypair();
    let template = compile_template("tests/templates/hello_world", &[]).unwrap();
    let expected_binary_hash = hash_template_code(template.code());
    let expected_template_address =
        PublishedTemplateAddress::from_author_and_binary_hash(&public_key.to_byte_type(), &expected_binary_hash);

    let result = test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .pay_fee_from_component(account_address, 200_000u64)
            .publish_template(template.into_code())
            .build_and_seal(&account_key),
        vec![owner_proof],
    );

    assert!(matches!(result.finalize.result, TransactionResult::Accept(_)));

    let mut template_found = false;
    let diff = result.expect_success();
    diff.up_iter().for_each(|(substate_id, substate)| {
        if let SubstateValue::Template(curr_template) = substate.substate_value() &&
            curr_template.to_binary_hash() == expected_binary_hash
        {
            template_found = true;
            assert!(matches!(substate_id, SubstateId::Template(_)));
            if let SubstateId::Template(curr_template_addr) = substate_id {
                assert_eq!(expected_template_address, *curr_template_addr);
            }
        }
    });

    assert!(template_found);
}

#[test]
fn publish_template_invalid_binary() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    let (account_address, owner_proof, account_key, _) = test.create_funded_account_with_keypair();
    let result = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .pay_fee_from_component(account_address, 200_000u64)
            // Main intent instruction #1
            .publish_template(vec![1u8, 2, 3])
            .build_and_seal(&account_key),
        vec![owner_proof],
    );

    let RejectReason::ExecutionFailure { code, message } = result else {
        panic!("expected an execution failure, got {result:?}");
    };
    assert_eq!(code, ExecutionFailureCode::TemplateError);
    assert!(message.starts_with("At instruction #1: Load template error:"));
}

#[test]
fn publish_template_too_big_binary() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    let (account_address, owner_proof, account_key, _) = test.create_funded_account_with_keypair();
    let random_wasm_binary = generate_random_binary(limits::ENGINE_LIMITS.max_template_binary_size_bytes + 1);
    let wasm_binary_size = random_wasm_binary.len();
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .pay_fee_from_component(account_address, 200_000u64)
            .publish_template(random_wasm_binary)
            .build_and_seal(&account_key),
        vec![owner_proof],
    );

    assert_reject_reason(reason, TransactionErrorKind::WasmBinaryTooBig {
        size: wasm_binary_size,
        max: limits::ENGINE_LIMITS.max_template_binary_size_bytes,
    });
}

fn generate_random_binary(size_in_bytes: usize) -> Vec<u8> {
    iter::repeat_with(random).take(size_in_bytes).collect()
}

/// A template's ABI comes from its `tari_tdef` custom section, so a binary without one carries no
/// template definition the engine can admit — including one embedding its ABI the legacy way, in
/// linear memory behind an `_ABI_TEMPLATE_DEF` global.
#[test]
fn publish_template_without_a_template_def_section() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    let (account_address, owner_proof, account_key, _) = test.create_funded_account_with_keypair();

    let code = wat::parse_str(
        r#"
        (module
          (memory (export "memory") 1)
          (global (export "_ABI_TEMPLATE_DEF") i32 (i32.const -1)))
        "#,
    )
    .unwrap();

    let result = test.execute_expect_failure(
        test.transaction()
            .pay_fee_from_component(account_address, 200_000u64)
            .publish_template(code)
            .build_and_seal(&account_key),
        vec![owner_proof],
    );

    assert_reject_reason(result, TEMPLATE_DEF_CUSTOM_SECTION);
}

/// A publish makes every validator Cranelift-compile the binary it carries, which is the most
/// expensive thing one instruction can ask for. It is charged before the compile runs, so a
/// transaction that cannot cover it does none of the work.
#[test]
fn the_compile_a_publish_pays_for_is_charged_before_it_runs() {
    use tari_engine_types::limits::template_compile_points;

    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    let (account, owner_proof, key, _) = test.create_funded_account_with_keypair();
    let template = compile_template("tests/templates/hello_world", &[]).unwrap();
    let binary_len = template.code().len() as u64;
    test.enable_fees();

    let result = test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .pay_fee_from_component(account, 2_000_000u64)
            .publish_template(template.into_code())
            .build_and_seal(&key),
        vec![owner_proof],
    );

    let native_points = result.native_execution_points;
    let compile = template_compile_points(binary_len);
    assert!(
        native_points >= compile,
        "a {binary_len}-byte publish charged {native_points} native points, under the {compile} its compile costs"
    );
}

/// Every admission rule the module bytes alone can answer runs before the compile is charged for,
/// so a module refused by one of them pays nothing for cranelift. Points accumulated before a
/// failure are still charged, so charging first would bill a large rejected binary for a compile
/// that never ran.
#[test]
fn a_module_refused_before_the_compile_is_not_charged_for_it() {
    use tari_engine_types::limits::template_compile_points;

    // Rejected by `validate_module_structure`, which needs no compile: the start section is refused
    // before cranelift is reached.
    let code = wat::parse_str(
        r#"
        (module
          (memory (export "memory") 3)
          (func (export "tari_alloc") (param i32) (result i32) (i32.const 1024))
          (func (export "tari_free") (param i32))
          (func (export "Buggy_main") (param i32 i32) (result i32) (i32.const 20))
          (func)
          (start 3)
        )
        "#,
    )
    .unwrap();

    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    let (account, owner_proof, key, _) = test.create_funded_account_with_keypair();
    test.enable_fees();

    let result = test
        .try_execute(
            Transaction::builder_localnet(Epoch(1))
                .pay_fee_from_component(account, 2_000_000u64)
                .publish_template(code.clone())
                .build_and_seal(&key),
            vec![owner_proof],
        )
        .expect("execution failed");

    assert!(
        result.native_execution_points < template_compile_points(code.len() as u64),
        "a module refused before the compile was charged {} native points",
        result.native_execution_points
    );
}
