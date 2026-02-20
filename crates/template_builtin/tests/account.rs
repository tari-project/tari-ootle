//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::args;
use tari_template_lib_types::constants::XTR;
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason};

#[test]
fn it_allows_badge_to_withdraw_from_account() {
    let mut test = TemplateTest::new_builtin_only();

    let (owner, owner_proof, owner_secret) = test.create_funded_account();
    let (user1, user1_proof, user1_secret) = test.create_funded_account();

    // Approve a withdrawal amount for user1
    test.execute_expect_success(
        test.transaction()
            .call_method(owner, "approve", args![user1_proof, XTR, 1000])
            .finish()
            .seal(&owner_secret),
        vec![owner_proof],
    );

    // Withdraw the approved amount
    test.execute_expect_success(
        test.transaction()
            // Create a proof that user1 signed the transaction.
            .call_method(user1, "create_ownership_proof", args![])
            .put_last_instruction_output_on_workspace("user1_proof")
            // Withdraw the approved amount
            .call_method(owner, "withdraw_from_approved", args![
                Workspace("user1_proof"),
                XTR,
                900
            ])
            .put_last_instruction_output_on_workspace("bucket1")
            .call_method(owner, "withdraw_from_approved", args![
                Workspace("user1_proof"),
                XTR,
                100
            ])
            .put_last_instruction_output_on_workspace("bucket2")
            .drop_all_proofs_in_workspace()
            // Deposit it into user1's account
            .call_method(user1, "deposit", args![Workspace("bucket1")])
            .call_method(user1, "deposit", args![Workspace("bucket2")])
            .finish()
            .seal(&user1_secret),
        vec![user1_proof],
    );
}

#[test]
fn it_rejects_withdrawals_greater_than_approval() {
    let mut test = TemplateTest::new_builtin_only();

    let (owner, owner_proof, owner_secret) = test.create_funded_account();
    let (user1, user1_proof, user1_secret) = test.create_empty_account();

    // Approve a withdrawal amount for user1
    test.execute_expect_success(
        test.transaction()
            .call_method(owner, "approve", args![user1_proof, XTR, 1000])
            .finish()
            .seal(&owner_secret),
        vec![owner_proof],
    );

    // Withdraw the more than the approved amount
    let reason = test.execute_expect_failure(
        test.transaction()
            // Create a proof that user1 signed the transaction.
            .call_method(user1, "create_ownership_proof", args![])
            .put_last_instruction_output_on_workspace("user1_proof")
            // Withdraw the approved amount
            .call_method(owner, "withdraw_from_approved", args![
                Workspace("user1_proof"),
                XTR,
                1001
            ])
            .drop_all_proofs_in_workspace()
            .put_last_instruction_output_on_workspace("withdrawn_bucket")
            // Deposit it into user1's account
            .call_method(user1, "deposit", args![Workspace("withdrawn_bucket")])
            .finish()
            .seal(&user1_secret),
        vec![user1_proof],
    );

    assert_reject_reason(reason, "Amount exceeds approval");

    let (user2, user2_proof, user2_secret) = test.create_empty_account();
    // Another user tries to withdraw the approved amount
    let reason = test.execute_expect_failure(
        test.transaction()
            // Create a proof that user1 signed the transaction.
            .call_method(user2, "create_ownership_proof", args![])
            .put_last_instruction_output_on_workspace("user2_proof")
            // Withdraw the approved amount
            .call_method(owner, "withdraw_from_approved", args![
                Workspace("user2_proof"),
                XTR,
                1
            ])
            .drop_all_proofs_in_workspace()
            .put_last_instruction_output_on_workspace("withdrawn_bucket")
            // Deposit it into user1's account
            .call_method(user2, "deposit", args![Workspace("withdrawn_bucket")])
            .finish()
            .seal(&user2_secret),
        vec![user2_proof],
    );

    assert_reject_reason(reason, "No approval for badge");
}

#[test]
fn it_clears_the_approval() {
    let mut test = TemplateTest::new_builtin_only();

    let (owner, owner_proof, owner_secret) = test.create_funded_account();
    let (user1, user1_proof, user1_secret) = test.create_funded_account();
    // Approve a withdrawal amount for user1
    test.execute_expect_success(
        test.transaction()
            .call_method(owner, "approve", args![user1_proof, XTR, 1000])
            .finish()
            .seal(&owner_secret),
        vec![owner_proof.clone()],
    );
    // Revoke all approvals
    test.execute_expect_success(
        test.transaction()
            .call_method(owner, "revoke_all_approvals", args![])
            .finish()
            .seal(&owner_secret),
        vec![owner_proof],
    );

    // Withdraw the approved amount
    let reason = test.execute_expect_failure(
        test.transaction()
            // Create a proof that user1 signed the transaction.
            .call_method(user1, "create_ownership_proof", args![])
            .put_last_instruction_output_on_workspace("user1_proof")
            // Withdraw the approved amount
            .call_method(owner, "withdraw_from_approved", args![
                Workspace("user1_proof"),
                XTR,
                1000
            ])
            .drop_all_proofs_in_workspace()
            .put_last_instruction_output_on_workspace("withdrawn_bucket")
            // Deposit it into user1's account
            .call_method(user1, "deposit", args![Workspace("withdrawn_bucket")])
            .finish()
            .seal(&user1_secret),
        vec![user1_proof],
    );

    assert_reject_reason(reason, "No approval for badge");
}
