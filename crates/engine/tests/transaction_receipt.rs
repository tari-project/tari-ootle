//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::{PrunedTransaction, Transaction, TransactionIntent, args};
use tari_template_test_tooling::TemplateTest;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const TEMPLATE_PATHS: [&str; 1] = ["tests/templates/state"];

/// The receipt of a committed transaction commits to that transaction's intent, so a holder of the
/// transaction — full or pruned — can link it to the receipt.
#[test]
fn committed_receipt_commits_to_the_transaction_intent() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let transaction = Transaction::builder_localnet()
        .call_function(test.get_template_address("State"), "new", args![])
        .build_and_seal(test.secret_key());

    let result = test.execute_expect_success(transaction.clone(), vec![]);
    let receipt = result.finalize.get_transaction_receipt().expect("receipt");

    assert!(receipt.outcome().is_commit());
    assert_eq!(transaction.verify_receipt_intent(receipt), Ok(()));
    assert_eq!(
        PrunedTransaction::from(transaction).verify_receipt_intent(receipt),
        Ok(()),
        "the pruned form must check against the same receipt",
    );
}

/// A transaction that fails after paying its fees still produces a receipt, and that receipt
/// commits to the same intent as a successful one would.
#[test]
fn fee_intent_receipt_commits_to_the_transaction_intent() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let (account, owner_token, private_key) = test.create_funded_account();
    test.enable_fees();

    let transaction = Transaction::builder_localnet()
        .pay_fee_from_component(account, 1000u64)
        .call_function(test.get_template_address("State"), "this_doesnt_exist", args![])
        .build_and_seal(&private_key);

    let result = test.execute_and_commit_on_success(transaction.clone(), vec![owner_token]);
    result.expect_failure();

    let receipt = result.finalize.get_transaction_receipt().expect("receipt");
    assert!(receipt.outcome().is_fee_intent_commit());
    assert_eq!(transaction.verify_receipt_intent(receipt), Ok(()));
}

/// A receipt only matches the intent that produced it: a different intent must not verify.
#[test]
fn receipt_does_not_match_a_different_intent() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let template = test.get_template_address("State");

    let transaction = Transaction::builder_localnet()
        .call_function(template, "new", args![])
        .build_and_seal(test.secret_key());
    let other = Transaction::builder_localnet()
        .call_function(template, "new", args![])
        .drop_all_proofs_in_workspace()
        .build_and_seal(test.secret_key());

    let result = test.execute_expect_success(transaction, vec![]);
    let receipt = result.finalize.get_transaction_receipt().expect("receipt");

    assert!(other.verify_receipt_intent(receipt).is_err());
}
