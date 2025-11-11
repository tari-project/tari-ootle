// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::iter;

use rand::random;
use tari_engine::{transaction::TransactionError, wasm::compile::compile_template};
use tari_engine_types::{
    commit_result::{RejectReason, TransactionResult},
    hashing::{hash_template_code, hasher32, EngineHashDomainLabel},
    limits,
    published_template::PublishedTemplateAddress,
    substate::{SubstateId, SubstateValue},
};
use tari_template_test_tooling::{support::assert_error::assert_reject_reason, TemplateTest};
use tari_transaction::{TemplateBlob, Transaction};

#[test]
fn publish_template_success() {
    let mut test = TemplateTest::new(Vec::<String>::new());
    let (account_address, owner_proof, account_key, public_key) = test.create_custom_funded_account(250_000);
    let template = compile_template("tests/templates/hello_world", &[]).unwrap();
    let expected_binary_hash = hash_template_code(template.code());
    let expected_template_address = PublishedTemplateAddress::from_hash(
        hasher32(EngineHashDomainLabel::TemplateAddress)
            .chain(&public_key)
            .chain(&expected_binary_hash)
            .result(),
    );

    let result = test.execute_expect_success(
        Transaction::builder()
            .fee_transaction_pay_from_component(account_address, 200_000)
            .publish_template(template.into_code().try_into().unwrap())
            .build_and_seal(&account_key),
        vec![owner_proof],
    );

    assert!(matches!(result.finalize.result, TransactionResult::Accept(_)));

    let mut template_found = false;
    if let TransactionResult::Accept(diff) = result.finalize.result {
        diff.up_iter().for_each(|(substate_id, substate)| {
            if let SubstateValue::Template(curr_template) = substate.substate_value() {
                if curr_template.binary_hash == expected_binary_hash {
                    template_found = true;
                    assert!(matches!(substate_id, SubstateId::Template(_)));
                    if let SubstateId::Template(curr_template_addr) = substate_id {
                        assert_eq!(expected_template_address, *curr_template_addr);
                    }
                }
            }
        });
    }

    assert!(template_found);
}

#[test]
fn publish_template_invalid_binary() {
    let mut test = TemplateTest::new(Vec::<String>::new());
    let (account_address, owner_proof, account_key, _) = test.create_custom_funded_account(250_000);
    let result = test.execute_expect_failure(
        Transaction::builder()
            .fee_transaction_pay_from_component(account_address, 200_000)
            .publish_template(vec![1, 2, 3].try_into().unwrap())
            .build_and_seal(&account_key),
        vec![owner_proof],
    );

    assert!(matches!(result, RejectReason::ExecutionFailure(_)));

    if let RejectReason::ExecutionFailure(error) = result {
        assert!(error.starts_with("Load template error:"));
    }
}

#[test]
fn publish_template_too_big_binary() {
    let mut test = TemplateTest::new(Vec::<String>::new());
    let (account_address, owner_proof, account_key, _) = test.create_custom_funded_account(250_000);
    let random_wasm_binary = generate_random_binary(limits::ENGINE_LIMITS.max_template_binary_size_bytes + 1);
    let wasm_binary_size = random_wasm_binary.len();
    let reason = test.execute_expect_failure(
        Transaction::builder()
            .fee_transaction_pay_from_component(account_address, 200_000)
            // SAFETY: We are intentionally publishing an oversized binary to test size limits.
            .publish_template(unsafe { TemplateBlob::new_unchecked(random_wasm_binary) })
            .build_and_seal(&account_key),
        vec![owner_proof],
    );

    assert_reject_reason(reason, TransactionError::WasmBinaryTooBig {
        size: wasm_binary_size,
        max: limits::ENGINE_LIMITS.max_template_binary_size_bytes,
    });
}

fn generate_random_binary(size_in_bytes: usize) -> Vec<u8> {
    iter::repeat_with(random).take(size_in_bytes).collect()
}
