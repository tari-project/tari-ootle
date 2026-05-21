//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::{commit_result::RejectReason, fees::FeeSource};
use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::{Amount, ComponentAddress, constants::STEALTH_TARI_RESOURCE_ADDRESS};
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason, xtr_faucet_component};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const TEMPLATE_PATHS: [&str; 1] = ["tests/templates/state"];

#[test]
fn deducts_fees_from_payments_and_refunds_the_rest() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let (account, owner_token, private_key) = test.create_funded_account();
    let orig_balance: Amount = test.call_method(account, "balance", args![STEALTH_TARI_RESOURCE_ADDRESS], vec![]);

    test.enable_fees();

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .pay_fee_from_component(account, 1000u64)
            .call_function(test.get_template_address("State"), "new", args![])
            .build_and_seal(&private_key),
        vec![owner_token],
    );

    test.disable_fees();

    // Check difference was refunded
    let payment = result.finalize.fee_receipt;
    let new_balance = test
        .read_only_state_store()
        .get_vaults_for_account(account)
        .unwrap()
        .get(&STEALTH_TARI_RESOURCE_ADDRESS)
        .unwrap()
        .balance();
    assert_eq!(new_balance, orig_balance - payment.total_fees_charged());
    assert_eq!(payment.total_refunded(), 1000 - payment.total_fees_charged());
    assert!(payment.is_paid_in_full());
}

#[test]
fn deducts_fees_when_transaction_fails() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let (account, owner_token, private_key) = test.create_funded_account();
    let vaults = test.read_only_state_store().get_vaults_for_account(account).unwrap();
    let orig_balance = vaults.get(&STEALTH_TARI_RESOURCE_ADDRESS).unwrap().balance();

    test.enable_fees();

    let result = test.execute_and_commit_on_success(
        Transaction::builder_localnet()
            .pay_fee_from_component(account, 1000u64)
            .call_function(test.get_template_address("State"), "this_doesnt_exist", args![])
            .build_and_seal(&private_key),
        vec![owner_token],
    );

    let reason = result.expect_failure();
    result.expect_finalization_success();
    assert!(matches!(reason, RejectReason::ExecutionFailure(_)));

    // Check the fee was still paid
    let payment = result.finalize.fee_receipt;
    let vaults = test.read_only_state_store().get_vaults_for_account(account).unwrap();
    let new_balance = vaults.get(&STEALTH_TARI_RESOURCE_ADDRESS).unwrap().balance();
    assert!(payment.total_fees_paid() > 0);
    assert_eq!(orig_balance - new_balance, payment.total_fees_paid());
}

#[test]
fn deposit_from_faucet_then_pay() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let (account, owner_token, private_key) = test.create_empty_account();

    test.enable_fees();

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .with_fee_instructions_builder(|builder| {
                builder
                    // Faucet deposits free coins into the account
                    .call_method(xtr_faucet_component(), "take", args![account])
                    .call_method(account, "pay_fee", args![1000])
            })
            .call_function(test.get_template_address("State"), "new", args![])
            .build_and_seal(&private_key),
        vec![owner_token],
    );

    let payment = result.finalize.fee_receipt;
    let new_balance = test
        .read_only_state_store()
        .get_vaults_for_account(account)
        .unwrap()
        .get(&STEALTH_TARI_RESOURCE_ADDRESS)
        .unwrap()
        .balance();
    assert_eq!(new_balance, 1_000_000_000 - payment.total_fees_paid());
}

#[test]
fn another_account_pays_partially_for_fees() {
    let mut test = TemplateTest::new_builtin_only();

    let (account, _, _) = test.create_empty_account();
    let (account_fee, owner_token_fee, _) = test.create_funded_account();
    let (account_fee2, owner_token_fee2, _) = test.create_funded_account();
    let orig_balance: Amount = test.call_method(account_fee, "balance", args![STEALTH_TARI_RESOURCE_ADDRESS], vec![]);

    test.enable_fees();

    // Faucet's cap must be smaller than the total fee for this transaction so that
    // account_fee2 is forced to cover the remainder (the point of this test).
    const FAUCET_CAP: u64 = 100;

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            // Faucet pays a little
            .pay_fee_from_component(account_fee, Amount::from(FAUCET_CAP))
            // Account pays the rest
            .pay_fee_from_component(account_fee2, Amount::from(1000u64))
            .call_method(xtr_faucet_component(), "take", args![account])
            // NOTE: the test harness provides the virtual proofs as provided, so the transaction signer does not matter
            .build_and_seal(test.secret_key()),
        vec![owner_token_fee, owner_token_fee2],
    );

    test.disable_fees();

    // Check difference was refunded
    let payment = result.finalize.fee_receipt;
    let vaults = test
        .read_only_state_store()
        .get_vaults_for_account(account_fee2)
        .unwrap();
    let vault = vaults.get(&STEALTH_TARI_RESOURCE_ADDRESS).unwrap();

    assert_eq!(
        vault.balance(),
        orig_balance + Amount::from(FAUCET_CAP) - Amount::from(payment.total_fees_charged())
    );
    // Check that this test is charging more than just the faucet's portion
    assert!(Amount::from(FAUCET_CAP) < payment.total_fees_charged());

    // Check the rest of the transaction was committed
    let balance = test
        .read_only_state_store()
        .get_vaults_for_account(account)
        .unwrap()
        .get(&STEALTH_TARI_RESOURCE_ADDRESS)
        .unwrap()
        .balance();
    assert_eq!(balance, Amount::from(1_000_000_000u64));
}

#[test]
fn failed_fee_transaction() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let (account, owner_token, private_key) = test.create_funded_account();
    let initial_balance: Amount = test.call_method(account, "balance", args![STEALTH_TARI_RESOURCE_ADDRESS], vec![]);

    test.enable_fees();
    let result = test
        .try_execute(
            Transaction::builder_localnet()
                .with_fee_instructions_builder(|builder| {
                    builder
                        // This instruction will fail
                        .call_method(account, "pay_da_fee_plz", args![])
                })
                .call_function(test.get_template_address("State"), "new", args![])
                .build_and_seal(&private_key),
            vec![owner_token],
        )
        .unwrap();

    let reason = result.expect_finalization_failure();
    assert!(matches!(reason, RejectReason::ExecutionFailure(_)));
    let reason = result.expect_failure();
    assert!(matches!(reason, RejectReason::ExecutionFailure(_)));

    let new_balance = test
        .read_only_state_store()
        .get_vaults_for_account(account)
        .unwrap()
        .get(&STEALTH_TARI_RESOURCE_ADDRESS)
        .unwrap()
        .balance();
    assert_eq!(new_balance, initial_balance);
}

#[test]
fn fail_partial_paid_fees() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let (account, owner_token, private_key) = test.create_funded_account();
    let (account2, owner_token2, _) = test.create_funded_account();
    let orig_balance: Amount = test.call_method(account, "balance", args![STEALTH_TARI_RESOURCE_ADDRESS], vec![]);
    test.enable_fees();

    // Must cover the fee section's own cost (so the fee instructions succeed) yet stay smaller
    // than the full transaction's fee (so InsufficientFeesPaid is triggered on commit).
    const FEE_PAID: u64 = 100;

    let result = test.execute_expect_commit(
        Transaction::builder_localnet()
            // Pay less fees than the cost of the main transaction
            .pay_fee_from_component(account, Amount::from(FEE_PAID))
            // These instructions should not be applied
            .call_method(account2, "withdraw", args![
                    STEALTH_TARI_RESOURCE_ADDRESS,
                    1000
                ])
            .put_last_instruction_output_on_workspace("bucket")
            .take_from_bucket("bucket", 500u64, "bucket2")
            .call_method(account, "deposit", args![Workspace("bucket")])
            .call_method(account, "deposit", args![Workspace("bucket2")])
            .build_and_seal(&private_key),
        vec![owner_token, owner_token2],
    );

    let total_fees = result.finalize.fee_receipt.fee_breakdown().get_total();
    // The fee charged exceeds the fee paid (so the transaction is rejected), but should not be far above it
    // since execution stops as soon as fees are determined insufficient.
    assert!(
        total_fees > FEE_PAID && total_fees < FEE_PAID * 3,
        "total fees: {total_fees}"
    );
    let reason = result.expect_failure();
    assert!(
        matches!(reason, RejectReason::InsufficientFeesPaid(_)),
        "actual reason: {reason}"
    );

    // Check that the fee paid was deducted
    let payment = result.finalize.fee_receipt;
    assert!(!payment.is_paid_in_full());
    let new_balance = test
        .read_only_state_store()
        .get_vaults_for_account(account)
        .unwrap()
        .get(&STEALTH_TARI_RESOURCE_ADDRESS)
        .unwrap()
        .balance();
    assert_eq!(new_balance, orig_balance - Amount::from(FEE_PAID));
}

#[test]
fn fail_pay_negative_fee() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let (account, owner_token, private_key) = test.create_funded_account();
    test.enable_fees();

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .with_fee_instructions_builder(|builder| builder.call_method(account, "pay_fee", args![-100]))
            .build_and_seal(&private_key),
        vec![owner_token],
    );

    assert!(
        matches!(reason, RejectReason::ExecutionFailure(_)),
        "actual reason: {reason}"
    );
}

#[test]
fn fail_pay_less_fees_than_fee_transaction() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let (account, owner_token, private_key) = test.create_funded_account();
    let (account2, owner_token2, _) = test.create_funded_account();
    let orig_balance: Amount = test.call_method(account, "balance", args![STEALTH_TARI_RESOURCE_ADDRESS], vec![]);
    let state: ComponentAddress = test.call_function("State", "new", args![], vec![]);

    test.enable_fees();

    let result = test
        .try_execute(
            Transaction::builder_localnet()
                .with_fee_instructions_builder(|builder| {
                    (0u32..=0).fold(builder, |builder, i| {
                        builder.call_method(
                            state,
                            "set".to_string(),
                            args![i],
                        )
                    })
                        .call_method(
                            account,
                            "pay_fee".to_string(),
                            // Lands between the fee-instructions cost and the full transaction
                            // cost, so the fee section is accepted while the rest fails for
                            // InsufficientFeesPaid.
                            args![150],
                        )

                })
                // These instructions should not be applied
                .call_method(account2, "withdraw", args![
                    STEALTH_TARI_RESOURCE_ADDRESS,
                    500
                ])
                .put_last_instruction_output_on_workspace("bucket")
                .call_method(account, "deposit", args![Workspace("bucket")])
                .build_and_seal(&private_key),
            vec![owner_token, owner_token2],
        )
        .unwrap();

    test.disable_fees();

    let (diff, reason) = result.expect_fee_accept_transaction_reject();
    assert_reject_reason(reason, RejectReason::InsufficientFeesPaid(String::new()));

    let (_, s) = diff.up_iter().find(|(id, _)| id.is_vault()).expect("Account not found");
    assert_eq!(
        s.substate_value().as_vault().unwrap().balance(),
        orig_balance - Amount::from(150u64)
    );

    // Fee was not deducted
    let new_balance = test
        .read_only_state_store()
        .get_vaults_for_account(account)
        .unwrap()
        .get(&STEALTH_TARI_RESOURCE_ADDRESS)
        .unwrap()
        .balance();
    assert_eq!(new_balance, orig_balance);

    // State was not updated. minicbor encodes enums as `[variant_index, [fields...]]`,
    // so State::Zero — a unit variant — is `[0, []]`.
    let component_state = test.read_only_state_store().get_component(state).unwrap();
    let arr = component_state
        .body
        .state
        .as_array()
        .expect("State should encode as a CBOR array");
    assert_eq!(arr.len(), 2, "expected [variant_index, [fields]]");
    assert_eq!(arr[0].as_integer(), Some(0), "expected the Zero variant tag");
    assert_eq!(
        arr[1].as_array().map(<[_]>::len),
        Some(0),
        "expected the unit variant to have an empty fields array"
    );
}

#[test]
fn fail_pay_too_little_no_fee_instruction() {
    let mut test = TemplateTest::new_builtin_only();

    let (account, owner_token, private_key) = test.create_funded_account();
    let (account2, owner_token2, _) = test.create_funded_account();
    let orig_balance: Amount = test.call_method(account, "balance", args![STEALTH_TARI_RESOURCE_ADDRESS], vec![]);

    test.enable_fees();

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .with_fee_instructions_builder(|builder| {
                builder
                    // These instructions should not be applied
                    .call_method(account2, "withdraw", args![
                        STEALTH_TARI_RESOURCE_ADDRESS,
                        500
                    ])
                    .put_last_instruction_output_on_workspace("bucket")
                    .call_method(account, "deposit", args![Workspace("bucket")])
                    .call_method(account, "pay_fee", args![10])
            })
            .build_and_seal(&private_key),
        vec![owner_token, owner_token2],
    );

    assert!(
        matches!(reason, RejectReason::InsufficientFeesPaid(_)),
        "actual reason: {reason}"
    );

    // Fee was not deducted
    let new_balance = test
        .read_only_state_store()
        .get_vaults_for_account(account)
        .unwrap()
        .get(&STEALTH_TARI_RESOURCE_ADDRESS)
        .unwrap()
        .balance();
    assert_eq!(new_balance, orig_balance);
}

#[test]
fn failure_pay_fee_in_main_instructions() {
    let mut test = TemplateTest::new_builtin_only();

    let (account, owner_token, private_key) = test.create_funded_account();

    test.enable_fees();

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            // Pay in fee intent, enough to pass this step
            .pay_fee_from_component(account, 100u64)
            // Call pay_fee in main instructions (outside fee instructions) not permitted
            .call_method(account, "pay_fee", args![100])
            .call_method(account, "balance", args![STEALTH_TARI_RESOURCE_ADDRESS])
            .call_method(account, "balance", args![STEALTH_TARI_RESOURCE_ADDRESS])
            .build_and_seal(&private_key),
        vec![owner_token],
    );

    assert_reject_reason(reason, RejectReason::FeePaymentInMainIntent);
}

#[test]
fn dangling_bucket_pay_fees() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let (account, owner_token, private_key) = test.create_funded_account();
    let orig_balance: Amount = test.call_method(account, "balance", args![STEALTH_TARI_RESOURCE_ADDRESS], vec![]);

    test.enable_fees();

    let result = test.execute_and_commit_on_success(
        Transaction::builder_localnet()
            .pay_fee_from_component(account, Amount::from(500u64))
            .call_method(account, "withdraw", args![STEALTH_TARI_RESOURCE_ADDRESS, 10])
            .put_last_instruction_output_on_workspace("dangling_bucket")
            .build_and_seal(&private_key),
        vec![owner_token],
    );

    // Check that the failure reason is actually the dangling bucket
    let reason = result.expect_failure();
    assert!(matches!(reason, RejectReason::ExecutionFailure(_)));
    assert!(reason.to_string().contains("dangling bucket"));

    // The transaction still finishes successfully
    result.expect_finalization_success();

    // Check the fee was still paid
    let payment = result.finalize.fee_receipt;
    let new_balance = test
        .read_only_state_store()
        .get_vaults_for_account(account)
        .unwrap()
        .get(&STEALTH_TARI_RESOURCE_ADDRESS)
        .unwrap()
        .balance();
    assert_ne!(payment.total_fees_paid(), 0);
    assert_eq!(orig_balance - new_balance, payment.total_fees_paid());
}

#[test]
fn template_load_fee_charged_once_per_template_per_transaction() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let (account, owner_token, private_key) = test.create_funded_account();
    let state: ComponentAddress = test.call_function("State", "new", args![], vec![]);

    test.enable_fees();

    // Single State call — establishes the baseline TemplateLoad fee for {Account, State}.
    let single = test.execute_expect_success(
        Transaction::builder_localnet()
            .pay_fee_from_component(account, 1000u64)
            .call_method(state, "set", args![1u32])
            .build_and_seal(&private_key),
        vec![owner_token.clone()],
    );

    // Five State calls — same template touched five extra times. Without dedup, TemplateLoad
    // would scale with call count; with dedup it must match the single-call baseline.
    let many = test.execute_expect_success(
        Transaction::builder_localnet()
            .pay_fee_from_component(account, 1000u64)
            .call_method(state, "set", args![1u32])
            .call_method(state, "set", args![2u32])
            .call_method(state, "set", args![3u32])
            .call_method(state, "get", args![])
            .call_method(state, "get", args![])
            .build_and_seal(&private_key),
        vec![owner_token],
    );

    let template_load = |result: &tari_engine_types::commit_result::FinalizeResult| -> u64 {
        result
            .fee_receipt
            .fee_breakdown()
            .iter()
            .find_map(|(s, amount)| (*s == FeeSource::TemplateLoad).then_some(*amount))
            .expect("TemplateLoad charge present")
    };

    let single_load = template_load(&single.finalize);
    let many_load = template_load(&many.finalize);
    assert!(single_load > 0, "TemplateLoad fee should be non-zero");
    assert_eq!(
        single_load, many_load,
        "TemplateLoad fee must be deduped per (template, transaction); single={single_load} many={many_load}",
    );
}
