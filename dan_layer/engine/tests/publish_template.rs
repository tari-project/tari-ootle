// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::iter;

use rand::random;
use tari_dan_engine::wasm::compile::compile_template;
use tari_engine_types::{
    commit_result::{RejectReason, TransactionResult},
    hashing::{hasher32, template_hasher32, EngineHashDomainLabel},
    published_template::PublishedTemplateAddress,
    substate::{SubstateId, SubstateValue},
};
use tari_template_lib::models::Amount;
use tari_template_test_tooling::TemplateTest;
use tari_transaction::Transaction;

#[test]
fn publish_template_success() {
    let mut test = TemplateTest::new(Vec::<String>::new());
    let (account_address, owner_proof, account_key, public_key) = test.create_custom_funded_account(Amount(250_000));
    let template = compile_template("tests/templates/hello_world", &[]).unwrap();
    let expected_binary_hash = template_hasher32().chain(template.code()).result();
    let expected_template_address = PublishedTemplateAddress::from_hash(
        hasher32(EngineHashDomainLabel::TemplateAddress)
            .chain(&public_key)
            .chain(&expected_binary_hash)
            .result(),
    );

    let result = test.execute_expect_success(
        Transaction::builder()
            .fee_transaction_pay_from_component(account_address, Amount(200_000))
            .publish_template(template.code().to_vec())
            .build_and_seal(&account_key),
        vec![owner_proof],
    );

    assert!(matches!(result.finalize.result, TransactionResult::Accept(_)));

    let mut template_found = false;
    if let TransactionResult::Accept(diff) = result.finalize.result {
        diff.up_iter().for_each(|(substate_id, substate)| {
            if let SubstateValue::Template(curr_template) = substate.substate_value() {
                if curr_template.binary.as_slice() == template.code() {
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
    let (account_address, owner_proof, account_key, _) = test.create_custom_funded_account(Amount(250_000));
    let result = test.execute_expect_failure(
        Transaction::builder()
            .fee_transaction_pay_from_component(account_address, Amount(200_000))
            .publish_template(vec![1, 2, 3])
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
    let (account_address, owner_proof, account_key, _) = test.create_custom_funded_account(Amount(250_000));
    let random_wasm_binary = generate_random_binary(6 * 1000 * 1000); // 6 MB
    let wasm_binary_size = random_wasm_binary.len();
    let result = test.execute_expect_failure(
        Transaction::builder()
            .fee_transaction_pay_from_component(account_address, Amount(200_000))
            .publish_template(random_wasm_binary)
            .build_and_seal(&account_key),
        vec![owner_proof],
    );

    assert!(matches!(result, RejectReason::ExecutionFailure(_)));

    if let RejectReason::ExecutionFailure(error) = result {
        assert_eq!(
            error,
            format!(
                "WASM binary too big! {} bytes are greater than allowed maximum 5000000 bytes.",
                wasm_binary_size
            )
        );
    }
}

fn generate_random_binary(size_in_bytes: usize) -> Vec<u8> {
    iter::repeat_with(random).take(size_in_bytes).collect()
}
