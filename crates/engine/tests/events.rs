//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine::runtime::RuntimeError;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{models::ComponentAddress, types::Amount};
use tari_template_test_tooling::{support::assert_error::assert_reject_reason, TemplateTest};
use tari_transaction::{args, call_args, Instruction, Transaction};

#[test]
fn basic_emit_event() {
    let mut template_test = TemplateTest::new(vec!["tests/templates/events"]);
    let event_emitter_template = template_test.get_template_address("EventEmitter");
    let topic = "Hello_world";
    let result = template_test
        .execute_and_commit(
            vec![Instruction::CallFunction {
                address: event_emitter_template,
                function: "test_function".to_string(),
                args: call_args![topic],
            }],
            vec![],
        )
        .expect("Failed to emit test event");
    assert!(result.finalize.is_accept());
    assert_eq!(result.finalize.events.len(), 1);
    assert_eq!(result.finalize.events[0].topic(), format!("EventEmitter.{}", topic));
    assert_eq!(result.finalize.events[0].template_address(), event_emitter_template);
    assert_eq!(result.finalize.events[0].substate_id(), None);
    assert_eq!(
        result.finalize.events[0].get_payload("my").unwrap(),
        "event".to_string()
    );
}

#[test]
fn cannot_use_standard_topic() {
    let mut template_test = TemplateTest::new(vec!["tests/templates/events"]);
    let event_emitter_template = template_test.get_template_address("EventEmitter");
    let (_, _, private_key) = template_test.create_funded_account();
    let invalid_topic = "std.mytopic";
    let reason = template_test.execute_expect_failure(
        Transaction::builder()
            .call_function(event_emitter_template, "test_function", args![invalid_topic])
            .build_and_seal(&private_key),
        [].into(),
    );
    assert_reject_reason(reason, RuntimeError::InvalidEventTopic {
        topic: invalid_topic.to_owned(),
        reason: "topics starting with 'std.' are reserved for standard events".to_owned(),
    });
}

#[test]
fn builtin_vault_events() {
    let mut template_test = TemplateTest::new(Vec::<&str>::new());

    // create a fungible resource for transfer
    let faucet_template = template_test.get_template_address("TestFaucet");
    let initial_supply = Amount::from(1_000_000_000_000u64);
    let result = template_test
        .execute_and_commit(
            vec![Instruction::CallFunction {
                address: faucet_template,
                function: "mint".to_string(),
                args: call_args![initial_supply],
            }],
            vec![template_test.owner_proof()],
        )
        .unwrap();
    let faucet_component: ComponentAddress = result.finalize.execution_results[0].decode().unwrap();
    let faucet_resource = result
        .finalize
        .result
        .expect("Faucet mint failed")
        .up_iter()
        .find_map(|(address, _)| address.as_resource_address())
        .unwrap();

    // Create sender and receiver accounts
    let (sender_address, sender_proof, _) = template_test.create_funded_account();
    let (receiver_address, _, _) = template_test.create_funded_account();
    template_test
        .build_and_execute(
            Transaction::builder()
                .call_method(faucet_component, "take_free_coins", args![])
                .put_last_instruction_output_on_workspace("free_coins")
                .call_method(sender_address, "deposit", args![Workspace("free_coins")]),
            vec![template_test.owner_proof()],
        )
        .expect_success();

    // transfer some tokens between accounts
    let amount = Amount::from(100);
    let result = template_test.build_and_execute(
        Transaction::builder()
            .call_method(sender_address, "withdraw", args![faucet_resource, amount])
            .put_last_instruction_output_on_workspace("foo_bucket")
            .call_method(receiver_address, "deposit", args![Workspace("foo_bucket")]),
        // Sender proof needed to withdraw
        vec![sender_proof],
    );
    result.expect_success();

    // a standard event for the withdraw must have been emmitted
    let event = result
        .finalize
        .events
        .iter()
        .find(|e| e.topic() == "std.vault.withdraw")
        .unwrap();
    assert_eq!(event.template_address(), ACCOUNT_TEMPLATE_ADDRESS);
    // assert_eq!(event.component_address().unwrap(), sender_address);
    assert_eq!(
        *event.payload().get("resource_address").unwrap(),
        faucet_resource.to_string()
    );
    assert_eq!(event.payload().get("resource_type").unwrap(), "Fungible");
    assert_eq!(event.payload().get("amount").unwrap(), amount.to_string());

    // a standard event for the deposit must have been emmitted
    let event = result
        .finalize
        .events
        .iter()
        .find(|e| e.topic() == "std.vault.deposit")
        .unwrap();
    assert_eq!(event.template_address(), ACCOUNT_TEMPLATE_ADDRESS);
    // assert_eq!(event.component_address().unwrap(), receiver_address);
    assert_eq!(
        event.payload().get("resource_address").unwrap(),
        faucet_resource.to_string()
    );
    assert_eq!(event.payload().get("resource_type").unwrap(), "Fungible");
    assert_eq!(event.payload().get("amount").unwrap(), amount.to_string());
}
