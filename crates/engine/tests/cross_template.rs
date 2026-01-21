//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine::runtime::{ActionIdent, RuntimeError};
use tari_engine_types::{
    commit_result::{ExecuteResult, RejectReason},
    limits,
};
use tari_ootle_transaction::{args, call_args, Instruction, Transaction};
use tari_template_lib::{
    models::{ComponentAddress, ResourceAddress},
    types::{Amount, TemplateAddress},
};
use tari_template_test_tooling::{
    support::assert_error::{assert_access_denied_for_action, assert_reject_reason},
    TemplateTest,
};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const TEMPLATE_NAME: &str = "CrossTemplate";

struct CrossTemplateTest {
    template_test: TemplateTest,
    cross_call_template: TemplateAddress,
    state_template: TemplateAddress,
}

impl CrossTemplateTest {
    fn secret_key(&self) -> &RistrettoSecretKey {
        self.template_test.secret_key()
    }
}

struct ComponentInfo {
    cross_template_component: ComponentAddress,
    state_component: ComponentAddress,
}

fn setup() -> CrossTemplateTest {
    let template_test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/cross_template",
        "tests/templates/state",
    ]);

    let cross_call_template = template_test.get_template_address(TEMPLATE_NAME);
    let state_template = template_test.get_template_address("State");

    CrossTemplateTest {
        template_test,
        cross_call_template,
        state_template,
    }
}

fn initialize_composability(test: &mut CrossTemplateTest) -> ComponentInfo {
    // the cross_template template "new" function should create a new "state" component as well
    let res = test
        .template_test
        .execute_and_commit(
            vec![Instruction::CallFunction {
                address: test.cross_call_template,
                function: "new".try_into().unwrap(),
                args: call_args![test.state_template],
            }],
            vec![],
        )
        .unwrap();

    // extract the newly created component addresses
    let composability_component = extract_component_address_from_result(&res, TEMPLATE_NAME);
    let state_component = extract_component_address_from_result(&res, "State");

    ComponentInfo {
        cross_template_component: composability_component,
        state_component,
    }
}

fn extract_component_address_from_result(result: &ExecuteResult, template_name: &str) -> ComponentAddress {
    let (substate_addr, _) = result
        .expect_success()
        .up_iter()
        .find(|(address, substate)| {
            address.is_component() && substate.substate_value().component().unwrap().module_name == template_name
        })
        .unwrap();
    substate_addr.as_component_address().unwrap()
}

fn create_resource_and_fund_account(test: &mut TemplateTest, account: ComponentAddress) -> ResourceAddress {
    // create a new fungible resource
    let faucet_template = test.get_template_address("TestFaucet");
    let initial_supply = Amount::from(1_000_000_000_000u64);

    test.execute_expect_success(
        Transaction::builder_localnet()
            .allocate_component_address("faucet_address")
            .call_function(faucet_template, "mint_with_opts", args![
                initial_supply,
                "TT".to_string(),
                Workspace("faucet_address")
            ])
            .call_method("faucet_address", "take_free_coins", args![])
            .put_last_instruction_output_on_workspace("free_coins")
            .call_method(account, "deposit", args![Workspace("free_coins")])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    let resources = test.read_only_state_store().get_all_resources().unwrap();
    let (faucet_resource, _) = resources
        .into_iter()
        .find(|(_, r)| r.token_symbol() == Some("TT"))
        .expect("Fungible resource not found");

    faucet_resource
}

#[test]
fn it_allows_function_to_function_calls() {
    let mut test = setup();
    let components = initialize_composability(&mut test);

    // the state component exists and is was correctly initialized by the cross_template component
    let value: u32 = test
        .template_test
        .call_method(components.state_component, "get", args![], vec![]);
    assert_eq!(value, 0);
}

#[test]
fn it_allows_function_to_method_calls() {
    let mut test = setup();
    let components = initialize_composability(&mut test);
    let composability_component_0 = components.cross_template_component;

    // create a new cross_template component, this time using a constructor that gets information from a method call
    let res = test.template_test.execute_expect_success(
        Transaction::builder_localnet()
            .allocate_component_address("component_1")
            .call_function(test.cross_call_template, "new_from_component", args![
                Workspace("component_1"),
                composability_component_0
            ])
            .call_method("component_1", "get_state_component_address", args![])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    // the cross_template component exists in the network and is correctly initialized
    let inner_component_address = res
        .finalize
        .execution_results
        .last()
        .unwrap()
        .decode::<ComponentAddress>()
        .unwrap();
    assert_eq!(inner_component_address, components.state_component);
}

#[test]
fn it_allows_method_to_method_calls() {
    let mut test = setup();
    let components = initialize_composability(&mut test);

    // perform the call to the cross_template component that will increase the counter
    test.template_test.call_method::<()>(
        components.cross_template_component,
        "increase_inner_state_component",
        args![],
        vec![],
    );

    // the state component has been increased
    let value: u32 = test
        .template_test
        .call_method(components.state_component, "get", args![], vec![]);
    assert_eq!(value, 1);
}

#[test]
fn it_allows_method_to_function_calls() {
    let mut test = setup();
    let components = initialize_composability(&mut test);
    let initial_state_component = components.state_component;

    // perform the call to the cross_template component that will increase the counter
    test.template_test.call_method::<()>(
        components.cross_template_component,
        "increase_inner_state_component",
        args![],
        vec![],
    );
    let value: u32 = test
        .template_test
        .call_method(initial_state_component, "get", args![], vec![]);
    assert_eq!(value, 1);

    // perform the call to replace the inner state component for a new one
    test.template_test.call_method::<()>(
        components.cross_template_component,
        "replace_state_component",
        call_args![test.state_template],
        vec![],
    );

    // a new state component should have been initialized
    let new_state_component: ComponentAddress = test.template_test.call_method(
        components.cross_template_component,
        "get_state_component_address",
        args![],
        vec![],
    );
    assert_ne!(new_state_component, initial_state_component);
    let value: u32 = test
        .template_test
        .call_method(new_state_component, "get", args![], vec![]);
    assert_eq!(value, 0);
}

#[test]
fn it_fails_on_invalid_calls() {
    let mut test = setup();
    let components = initialize_composability(&mut test);
    let (_, _, private_key) = test.template_test.create_empty_account();

    // the "call_method_that_does_not_exist" method tries to call a non-existent method in the inner state component
    let result = test
        .template_test
        .try_execute(
            Transaction::builder_localnet()
                .call_method(
                    components.cross_template_component,
                    "call_method_that_does_not_exist",
                    args![],
                )
                .build_and_seal(&private_key),
            vec![],
        )
        .unwrap();
    let reason = result.expect_failure();

    // TODO: inner errors are not properly propagated up, they all end up being "Engine call returned null for op
    // CallInvoke" we should be able to assert a more specific error cause
    assert!(matches!(reason, RejectReason::ExecutionFailure(_)));
}

#[test]
fn it_does_not_propagate_permissions() {
    let mut test = setup();

    // Create a component owned by the test identity
    let components = initialize_composability(&mut test);

    let (attacker_proof, attacker_key) = test.template_test.get_test_proof_and_secret_key();
    let (victim_account, victim_proof, _) = test.template_test.create_empty_account();

    // create_resource_and_fund_account
    let fungible_resource = create_resource_and_fund_account(&mut test.template_test, victim_account);

    // The attacker owns the cross_template component and so attempts to call another account's withdraw method
    // hoping that ownership is preserved.
    let result = test
        .template_test
        .try_execute(
            Transaction::builder_localnet()
                .call_method(components.cross_template_component, "malicious_withdraw", args![
                    victim_account,
                    fungible_resource,
                    Amount(100)
                ])
                .build_and_seal(&attacker_key),
            // note that we are actually passing a valid proof of ownership for the victim. In reality, the engine only
            // passes the transaction signer proof but we are assuming the attacker was able to somehow
            // inject into the auth scope so that we can assert that proofs are not propagated in cross-template calls.
            vec![attacker_proof, victim_proof],
        )
        .unwrap();
    let reason = result.expect_failure();

    // We expect the component call to withdraw to fail with AccessDenied.
    assert_access_denied_for_action(reason, ActionIdent::ComponentCallMethod {
        component_address: victim_account,
        method: "withdraw".to_string(),
    });
}

#[test]
fn it_allows_multiple_recursion_levels() {
    let mut test = setup();

    // composability_0
    let mut components = initialize_composability(&mut test);
    let composability_0 = components.cross_template_component;

    // composability_1 has composability_0 nested
    components = initialize_composability(&mut test);
    let composability_1 = components.cross_template_component;
    test.template_test.call_method::<()>(
        composability_1,
        "set_nested_composability",
        call_args![composability_0],
        vec![],
    );

    // we have now: composability_1 -> composability_0 -> state
    // we want to access the innermost level from the outermost level
    let value: u32 = test
        .template_test
        .call_method(composability_1, "get_nested_value", args![], vec![]);
    assert_eq!(value, 0);
}

#[test]
fn it_fails_when_surpassing_recursion_limit_with_many_nested_components() {
    let mut test = setup();
    let (_, _, private_key) = test.template_test.create_empty_account();
    let max_call_depth = limits::ENGINE_LIMITS.max_call_depth;

    // innermost cross_template component
    let mut components = initialize_composability(&mut test);
    let mut last_composability_component = components.cross_template_component;

    for _ in 0..max_call_depth {
        components = initialize_composability(&mut test);
        test.template_test.call_method::<()>(
            components.cross_template_component,
            "set_nested_composability",
            call_args![last_composability_component],
            vec![],
        );
        last_composability_component = components.cross_template_component;
    }

    // we have now nested more components than the recursion depth limit allows
    // se when we do a call that goes from the outermost to the innermost, it must fail
    let result = test
        .template_test
        .try_execute(
            Transaction::builder_localnet()
                .call_method(last_composability_component, "get_nested_value", args![])
                .build_and_seal(&private_key),
            vec![],
        )
        .unwrap();
    let reason = result.expect_failure();
    assert_reject_reason(reason, RuntimeError::MaxCallDepthExceeded {
        max_depth: max_call_depth,
    });
}

#[test]
fn it_fails_when_surpassing_recursion_limit() {
    let mut test = setup();
    let (_, _, private_key) = test.template_test.create_empty_account();
    let max_call_depth = limits::ENGINE_LIMITS.max_call_depth;

    let components = initialize_composability(&mut test);

    test.template_test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(components.cross_template_component, "recursion", args![
                // -1 to account for the initial call
                max_call_depth - 1
            ])
            .build_and_seal(&private_key),
        vec![],
    );
    let reason = test.template_test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_method(components.cross_template_component, "recursion", args![max_call_depth])
            .build_and_seal(&private_key),
        vec![],
    );

    assert_reject_reason(reason, RuntimeError::MaxCallDepthExceeded {
        max_depth: max_call_depth,
    });
}
