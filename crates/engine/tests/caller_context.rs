//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_engine::runtime::RuntimeError;
use tari_ootle_transaction::args;
use tari_template_lib::types::{NonFungibleAddress, crypto::RistrettoPublicKeyBytes};
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
fn it_finds_every_signer_badge_in_a_directly_called_frame() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let template = test.get_template_address(TEMPLATE_NAME);
    let seal_pk = test.to_public_key_bytes();
    let (sk, pk) = test.new_key_pair(2);
    let pk: RistrettoPublicKeyBytes = pk.to_byte_type();

    test.execute_expect_success(
        test.transaction()
            .call_function(template, "assert_badge_in_scope", args![
                NonFungibleAddress::from_public_key(seal_pk)
            ])
            .call_function(template, "assert_badge_in_scope", args![
                NonFungibleAddress::from_public_key(pk)
            ])
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
    let badge = NonFungibleAddress::from_public_key(pk.to_byte_type());

    let reason = test.execute_expect_failure(
        test.transaction()
            .call_function(template, "assert_badge_in_scope", args![badge])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, RuntimeError::BadgeNotInScope { badge });
}

#[test]
fn it_fails_if_signer_is_not_in_scope() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let template = test.get_template_address(TEMPLATE_NAME);
    let badge = NonFungibleAddress::from_public_key(test.to_public_key_bytes());

    // Signer badges are stamped on the transaction's direct call only, so the frame that call pushes does not carry
    // them even though the transaction is signed by the test's main key.
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_function(template, "assert_badge_in_scope_cross_template", args![template, badge])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, RuntimeError::BadgeNotInScope { badge });
}

#[test]
fn it_finds_the_direct_caller_badge_in_a_pushed_frame() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let template = test.get_template_address(TEMPLATE_NAME);

    test.execute_expect_success(
        test.transaction()
            .call_function(template, "assert_badge_in_scope_cross_template", args![
                template,
                NonFungibleAddress::direct_caller_template_badge(template)
            ])
            .build_and_seal(test.secret_key()),
        vec![],
    );
}
