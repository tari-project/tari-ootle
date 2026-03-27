//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_ootle_transaction::args;
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason};
use tari_utilities::hex::from_hex;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const TEMPLATE_PATHS: &[&str] = &["tests/templates/caller_context"];
const TEMPLATE_NAME: &str = "CallerContextTest";

#[test]
fn it_returns_the_tx_signer() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let template = test.get_template_address(TEMPLATE_NAME);
    let expected_pk = test.to_public_key_bytes();

    let result = test.execute_expect_success(
        test.transaction()
            .call_function(template, "log_caller_pk", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let pk = &result.finalize.logs[0].message;
    assert_eq!(from_hex(pk).unwrap(), expected_pk.as_bytes());
}

#[test]
fn it_returns_signer_proofs() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let template = test.get_template_address(TEMPLATE_NAME);
    let seal_pk = test.to_public_key_bytes();
    let (sk, pk) = test.new_key_pair(2);

    test.execute_expect_success(
        test.transaction()
            .call_function(template, "main_signer_proof", args![])
            .put_last_instruction_output_on_workspace("proof")
            .call_function(template, "check_pk_proof", args![Workspace("proof"), seal_pk])
            .call_function(template, "signer_proof_for_pk", args![pk.to_byte_type()])
            .put_last_instruction_output_on_workspace("proof2")
            .call_function(template, "check_pk_proof", args![
                Workspace("proof2"),
                pk.to_byte_type()
            ])
            .drop_all_proofs_in_workspace()
            .add_signer(&seal_pk, &sk)
            .seal(test.secret_key()),
        vec![],
    );
}

#[test]
fn it_fails_if_not_signed() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let template = test.get_template_address(TEMPLATE_NAME);
    let (_sk, pk) = test.new_key_pair(2);

    let reason = test.execute_expect_failure(
        test.transaction()
            .call_function(template, "signer_proof_for_pk", args![pk.to_byte_type()])
            .put_last_instruction_output_on_workspace("proof2")
            .call_function(template, "check_pk_proof", args![
                Workspace("proof2"),
                pk.to_byte_type()
            ])
            .drop_all_proofs_in_workspace()
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, "out of scope signer badge");
}

#[test]
fn it_fails_if_signer_is_not_in_scope() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let template = test.get_template_address(TEMPLATE_NAME);

    let reason = test.execute_expect_failure(
        test.transaction()
            // Not in scope for cross template calls, so this will fail even though the transaction is signed by the test's main key
            .call_function(template, "call_using_cross_template", args![template])
            .put_last_instruction_output_on_workspace("proof")
            .drop_all_proofs_in_workspace()
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, "out of scope signer badge");
}
