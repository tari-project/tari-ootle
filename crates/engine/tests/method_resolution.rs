//   Copyright 2026. The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A `CallFunction` resolves only functions without a component receiver, and a `CallMethod` resolves only methods
//! with one. A method runs in its component's frame after the component's access rules, which a static call skips.

use tari_engine::runtime::ActionIdent;
use tari_engine_types::commit_result::ExecutionFailureCode;
use tari_ootle_transaction::args;
use tari_template_lib::types::{ComponentAddress, TemplateAddress};
use tari_template_test_tooling::{
    TemplateTest,
    crypto::RistrettoSecretKey,
    support::assert_error::{assert_access_denied_for_action, assert_reject_reason_with_code},
};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

struct Setup {
    test: TemplateTest,
    template: TemplateAddress,
    component: ComponentAddress,
    stranger_account: ComponentAddress,
    stranger_key: RistrettoSecretKey,
}

fn setup() -> Setup {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/method_resolution"]);
    let template = test.get_template_address("MethodResolution");

    test.execute_expect_success(
        test.transaction()
            .call_function(template, "create", args![template])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    let component: ComponentAddress = test.return_values()[0].decoded().unwrap();

    let (stranger_account, _proof, stranger_key) = test.create_empty_account();
    Setup {
        test,
        template,
        component,
        stranger_account,
        stranger_key,
    }
}

#[test]
fn call_function_rejects_a_method() {
    let mut s = setup();

    let reason = s.test.execute_expect_failure(
        s.test
            .transaction()
            .call_method(s.component, "get_value", args![])
            .build_and_seal(&s.stranger_key),
        vec![],
    );
    assert_access_denied_for_action(reason, ActionIdent::ComponentCallMethod {
        component_address: s.component,
        method: "get_value".to_string(),
    });

    let reason = s.test.execute_expect_failure(
        s.test
            .transaction()
            .call_function(s.template, "get_value", args![s.component])
            .build_and_seal(&s.stranger_key),
        vec![],
    );
    assert_reject_reason_with_code(
        reason,
        "takes a component receiver",
        ExecutionFailureCode::InvalidArgument,
    );
}

#[test]
fn call_function_cannot_run_a_method_with_the_templates_authority() {
    let mut s = setup();

    let reason = s.test.execute_expect_failure(
        s.test
            .transaction()
            .call_function(s.template, "mint_more", args![s.component, 500u64])
            .put_last_instruction_output_on_workspace("minted")
            .call_method(s.stranger_account, "deposit", args![Workspace("minted")])
            .build_and_seal(&s.stranger_key),
        vec![],
    );
    assert_reject_reason_with_code(
        reason,
        "takes a component receiver",
        ExecutionFailureCode::InvalidArgument,
    );
}

#[test]
fn call_method_rejects_a_function() {
    let mut s = setup();

    let reason = s.test.execute_expect_failure(
        s.test
            .transaction()
            .call_method(s.component, "version", args![])
            .build_and_seal(&s.stranger_key),
        vec![],
    );
    assert_reject_reason_with_code(
        reason,
        "has no component receiver",
        ExecutionFailureCode::InvalidArgument,
    );
}

#[test]
fn functions_and_methods_resolve_through_their_own_instruction() {
    let mut s = setup();

    s.test.execute_expect_success(
        s.test
            .transaction()
            .call_function(s.template, "version", args![])
            .build_and_seal(&s.stranger_key),
        vec![],
    );
    let version: u32 = s.test.return_values()[0].decoded().unwrap();
    assert_eq!(version, 1);

    s.test.execute_expect_success(
        s.test
            .transaction()
            .call_method(s.component, "get_value", args![])
            .build_and_seal(s.test.secret_key()),
        vec![],
    );
    let value: u32 = s.test.return_values()[0].decoded().unwrap();
    assert_eq!(value, 7);
}
