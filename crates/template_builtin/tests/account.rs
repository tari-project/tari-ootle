//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::args;
use tari_template_lib_types::{Amount, ComponentAddress, NonFungibleAddress, ResourceAddress, constants::TARI_TOKEN};
use tari_template_test_tooling::{
    TemplateTest,
    crypto::RistrettoSecretKey,
    support::assert_error::assert_reject_reason,
};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const NEW_RESOURCE_DEPOSITOR: &str = "tests/new_resource_depositor";
const APPROVAL_SPENDER: &str = "tests/approval_spender";

/// Mirrors `MAX_VAULTS` in the account template.
const MAX_ACCOUNT_VAULTS: u32 = 1024;

#[test]
fn it_allows_badge_to_withdraw_from_account() {
    let mut test = TemplateTest::new_builtin_only();

    let (owner, owner_proof, owner_secret) = test.create_funded_account();
    let (user1, user1_proof, user1_secret) = test.create_funded_account();

    // Approve a withdrawal amount for user1
    test.execute_expect_success(
        test.transaction()
            .call_method(owner, "approve", args![user1_proof, TARI_TOKEN, 1000])
            .finish()
            .seal(&owner_secret),
        vec![owner_proof],
    );

    // Withdraw the approved amount. user1 signs the transaction, so its badge is stamped on each direct call.
    test.execute_expect_success(
        test.transaction()
            .call_method(owner, "withdraw_approved", args![user1_proof, TARI_TOKEN, 900])
            .put_last_instruction_output_on_workspace("bucket1")
            .call_method(owner, "withdraw_approved", args![user1_proof, TARI_TOKEN, 100])
            .put_last_instruction_output_on_workspace("bucket2")
            // Deposit it into user1's account
            .call_method(user1, "deposit", args![Workspace("bucket1")])
            .call_method(user1, "deposit", args![Workspace("bucket2")])
            .finish()
            .seal(&user1_secret),
        vec![user1_proof],
    );
}

#[test]
fn the_withdraw_approved_event_names_the_spending_badge() {
    let mut test = TemplateTest::new_builtin_only();

    let (owner, owner_proof, owner_secret) = test.create_funded_account();
    let (user1, user1_proof, user1_secret) = test.create_empty_account();

    test.execute_expect_success(
        test.transaction()
            .call_method(owner, "approve", args![user1_proof, TARI_TOKEN, 1000])
            .finish()
            .seal(&owner_secret),
        vec![owner_proof],
    );

    let result = test.execute_expect_success(
        test.transaction()
            .call_method(owner, "withdraw_approved", args![user1_proof, TARI_TOKEN, 1000])
            .put_last_instruction_output_on_workspace("bucket")
            .call_method(user1, "deposit", args![Workspace("bucket")])
            .finish()
            .seal(&user1_secret),
        vec![user1_proof.clone()],
    );

    let event = result
        .finalize
        .events
        .iter()
        .find(|e| e.topic().ends_with("withdraw_approved"))
        .expect("no withdraw_approved event");

    // The badge is the whole non-fungible address, matching what `approve` recorded. Its resource
    // address alone does not identify which badge spent the approval.
    let payload = event.payload();
    assert_eq!(
        payload.get_as::<NonFungibleAddress>("spender_badge").unwrap(),
        Some(user1_proof)
    );
    assert_eq!(payload.get_as::<ResourceAddress>("resource").unwrap(), Some(TARI_TOKEN));
    assert_eq!(payload.get_as::<Amount>("amount").unwrap(), Some(Amount::from(1000u64)));
}

#[test]
fn it_rejects_withdrawals_greater_than_approval() {
    let mut test = TemplateTest::new_builtin_only();

    let (owner, owner_proof, owner_secret) = test.create_funded_account();
    let (user1, user1_proof, user1_secret) = test.create_empty_account();

    // Approve a withdrawal amount for user1
    test.execute_expect_success(
        test.transaction()
            .call_method(owner, "approve", args![user1_proof, TARI_TOKEN, 1000])
            .finish()
            .seal(&owner_secret),
        vec![owner_proof],
    );

    // Withdraw the more than the approved amount
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(owner, "withdraw_approved", args![user1_proof, TARI_TOKEN, 1001])
            .put_last_instruction_output_on_workspace("withdrawn_bucket")
            // Deposit it into user1's account
            .call_method(user1, "deposit", args![Workspace("withdrawn_bucket")])
            .finish()
            .seal(&user1_secret),
        vec![user1_proof.clone()],
    );

    assert_reject_reason(reason, "Amount exceeds approval");

    let (user2, user2_proof, user2_secret) = test.create_empty_account();
    // Another user tries to withdraw under its own badge, which has no approval
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(owner, "withdraw_approved", args![user2_proof, TARI_TOKEN, 1])
            .put_last_instruction_output_on_workspace("withdrawn_bucket")
            .call_method(user2, "deposit", args![Workspace("withdrawn_bucket")])
            .finish()
            .seal(&user2_secret),
        vec![user2_proof.clone()],
    );

    assert_reject_reason(reason, "No approval for badge");

    // ...and under user1's badge, which its transaction does not carry
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(owner, "withdraw_approved", args![user1_proof, TARI_TOKEN, 1])
            .put_last_instruction_output_on_workspace("withdrawn_bucket")
            .call_method(user2, "deposit", args![Workspace("withdrawn_bucket")])
            .finish()
            .seal(&user2_secret),
        vec![user2_proof],
    );

    assert_reject_reason(reason, format!("This call does not carry the badge {user1_proof}"));
}

#[test]
fn it_clears_the_approval() {
    let mut test = TemplateTest::new_builtin_only();

    let (owner, owner_proof, owner_secret) = test.create_funded_account();
    let (user1, user1_proof, user1_secret) = test.create_funded_account();
    // Approve a withdrawal amount for user1
    test.execute_expect_success(
        test.transaction()
            .call_method(owner, "approve", args![user1_proof, TARI_TOKEN, 1000])
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
            .call_method(owner, "withdraw_approved", args![user1_proof, TARI_TOKEN, 1000])
            .put_last_instruction_output_on_workspace("withdrawn_bucket")
            // Deposit it into user1's account
            .call_method(user1, "deposit", args![Workspace("withdrawn_bucket")])
            .finish()
            .seal(&user1_secret),
        vec![user1_proof],
    );

    assert_reject_reason(reason, "No approval for badge");
}

struct ApprovalFixture {
    test: TemplateTest,
    granter: ComponentAddress,
    granter_badge: NonFungibleAddress,
    granter_secret: RistrettoSecretKey,
    spender: ComponentAddress,
    spender_badge: NonFungibleAddress,
    spender_secret: RistrettoSecretKey,
    recipient: ComponentAddress,
    component: ComponentAddress,
}

/// The granter approves the spender's public-key badge for 100 TARI, and an `ApprovalSpender` component exists for
/// the spender to call.
fn approval_fixture() -> ApprovalFixture {
    let mut test = TemplateTest::new(CRATE_PATH, [APPROVAL_SPENDER]);

    let (granter, granter_badge, granter_secret) = test.create_funded_account();
    let (spender, spender_badge, spender_secret) = test.create_empty_account();
    let (recipient, _, _) = test.create_empty_account();
    test.execute_expect_success(
        test.transaction()
            .call_method(granter, "approve", args![spender_badge.clone(), TARI_TOKEN, 100])
            .finish()
            .seal(&granter_secret),
        vec![granter_badge.clone()],
    );
    let component: ComponentAddress = test.call_function("ApprovalSpender", "new", args![], vec![]);

    ApprovalFixture {
        test,
        granter,
        granter_badge,
        granter_secret,
        spender,
        spender_badge,
        spender_secret,
        recipient,
        component,
    }
}

fn balance(test: &mut TemplateTest, account: ComponentAddress) -> Amount {
    test.call_method(account, "balance", args![TARI_TOKEN], vec![])
}

#[test]
fn a_called_component_cannot_spend_an_approval_with_a_minted_signer_proof() {
    let ApprovalFixture {
        mut test,
        granter,
        spender_badge,
        spender_secret,
        recipient,
        component,
        ..
    } = approval_fixture();
    let granter_before = balance(&mut test, granter);

    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(component, "spend_with_signer_proof", args![
                granter, TARI_TOKEN, 60, recipient
            ])
            .finish()
            .seal(&spender_secret),
        vec![spender_badge],
    );

    assert_reject_reason(reason, "Failed to decode argument for engine call");
    assert_eq!(balance(&mut test, granter), granter_before);
}

#[test]
fn a_called_component_cannot_spend_an_approval_with_a_minted_owner_proof() {
    let ApprovalFixture {
        mut test,
        granter,
        spender,
        spender_badge,
        spender_secret,
        recipient,
        component,
        ..
    } = approval_fixture();
    let granter_before = balance(&mut test, granter);

    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(component, "spend_with_owner_proof", args![
                spender, granter, TARI_TOKEN, 60, recipient
            ])
            .finish()
            .seal(&spender_secret),
        vec![spender_badge],
    );

    assert_reject_reason(reason, "Failed to decode argument for engine call");
    assert_eq!(balance(&mut test, granter), granter_before);
}

#[test]
fn a_called_component_cannot_spend_an_approval_naming_its_signer() {
    let ApprovalFixture {
        mut test,
        granter,
        spender_badge,
        spender_secret,
        recipient,
        component,
        ..
    } = approval_fixture();
    let granter_before = balance(&mut test, granter);

    // The signer's badge is stamped on the component's frame only; the component's call to the granter carries the
    // component's own caller badges.
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(component, "spend_naming_badge", args![
                granter,
                spender_badge,
                TARI_TOKEN,
                60,
                recipient
            ])
            .finish()
            .seal(&spender_secret),
        vec![spender_badge.clone()],
    );

    assert_reject_reason(reason, format!("This call does not carry the badge {spender_badge}"));
    assert_eq!(balance(&mut test, granter), granter_before);
}

#[test]
fn a_component_spends_an_approval_naming_its_caller_badge() {
    let ApprovalFixture {
        mut test,
        granter,
        granter_badge,
        granter_secret,
        spender_badge,
        spender_secret,
        component,
        ..
    } = approval_fixture();
    let component_badge = NonFungibleAddress::caller_component_badge(component);
    test.execute_expect_success(
        test.transaction()
            .call_method(granter, "approve", args![component_badge, TARI_TOKEN, 100])
            .finish()
            .seal(&granter_secret),
        vec![granter_badge],
    );
    let granter_before = balance(&mut test, granter);

    test.execute_expect_success(
        test.transaction()
            .call_method(component, "pull", args![granter, TARI_TOKEN, 60])
            .finish()
            .seal(&spender_secret),
        vec![spender_badge.clone()],
    );
    assert_eq!(balance(&mut test, granter), granter_before - Amount::from(60u64));

    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(component, "pull", args![granter, TARI_TOKEN, 41])
            .finish()
            .seal(&spender_secret),
        vec![spender_badge],
    );
    assert_reject_reason(reason, "Amount exceeds approval (max: 40, attempted: 41)");
}

#[test]
fn a_stored_badge_approval_is_spent_with_a_proof_of_the_badge() {
    let ApprovalFixture {
        mut test,
        granter,
        granter_badge,
        granter_secret,
        spender,
        spender_badge,
        spender_secret,
        ..
    } = approval_fixture();
    let stored_badge: NonFungibleAddress =
        test.call_function("ApprovalSpender", "mint_badge_into", args![spender], vec![]);
    test.execute_expect_success(
        test.transaction()
            .call_method(granter, "approve", args![stored_badge.clone(), TARI_TOKEN, 100])
            .finish()
            .seal(&granter_secret),
        vec![granter_badge],
    );
    let granter_before = balance(&mut test, granter);

    // Naming the badge without showing a proof of it is refused.
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(granter, "withdraw_approved", args![
                stored_badge.clone(),
                TARI_TOKEN,
                60
            ])
            .put_last_instruction_output_on_workspace("bucket")
            .call_method(spender, "deposit", args![Workspace("bucket")])
            .finish()
            .seal(&spender_secret),
        vec![spender_badge.clone()],
    );
    assert_reject_reason(reason, format!("This call does not carry the badge {stored_badge}"));

    test.execute_expect_success(
        test.transaction()
            .call_method(spender, "create_proof_by_non_fungible", args![stored_badge.clone()])
            .put_last_instruction_output_on_workspace("badge")
            .with_authorized_proof("badge", |builder| {
                builder
                    .call_method(granter, "withdraw_approved", args![stored_badge, TARI_TOKEN, 60])
                    .put_last_instruction_output_on_workspace("bucket")
            })
            .drop_all_proofs_in_workspace()
            .call_method(spender, "deposit", args![Workspace("bucket")])
            .finish()
            .seal(&spender_secret),
        vec![spender_badge],
    );
    assert_eq!(balance(&mut test, granter), granter_before - Amount::from(60u64));
}

/// Has a third party deposit `count` newly created resources into `account`, one vault each.
fn deposit_new_resources(test: &mut TemplateTest, account: ComponentAddress, count: u32) {
    let depositor = test.get_template_address("NewResourceDepositor");
    // Every deposit re-encodes the account's vault map, so batches stay small enough for the per-transaction compute
    // limit as the map grows.
    let mut remaining = count;
    while remaining > 0 {
        let batch = remaining.min(20);
        test.execute_expect_success(
            test.transaction()
                .call_function(depositor, "deposit_new_resources", args![account, batch])
                .finish()
                .seal(test.secret_key()),
            vec![],
        );
        remaining -= batch;
    }
}

fn account_vault_count(test: &mut TemplateTest, account: ComponentAddress) -> usize {
    let balances: Vec<(ResourceAddress, Amount)> = test.call_method(account, "get_balances", args![], vec![]);
    balances.len()
}

/// Returns the WASM points of a read-only balance query and of an owner withdrawal on `account`.
fn read_and_withdraw_points(
    test: &mut TemplateTest,
    account: ComponentAddress,
    proof: &NonFungibleAddress,
    secret: &RistrettoSecretKey,
    recipient: ComponentAddress,
) -> (u64, u64) {
    let _balance: Amount = test.call_method(account, "balance", args![TARI_TOKEN], vec![]);
    let read = test.last_execution_points().wasm;
    test.execute_expect_success(
        test.transaction()
            .call_method(account, "withdraw", args![TARI_TOKEN, 1])
            .put_last_instruction_output_on_workspace("bucket")
            .call_method(recipient, "deposit", args![Workspace("bucket")])
            .finish()
            .seal(secret),
        vec![proof.clone()],
    );
    (read, test.last_execution_points().wasm)
}

#[test]
fn owner_withdrawal_cost_grows_no_faster_than_a_read() {
    let mut test = TemplateTest::new(CRATE_PATH, [NEW_RESOURCE_DEPOSITOR]);

    let (owner, owner_proof, owner_secret) = test.create_funded_account();
    let (recipient, _, _) = test.create_empty_account();

    let (read_before, withdraw_before) =
        read_and_withdraw_points(&mut test, owner, &owner_proof, &owner_secret, recipient);
    deposit_new_resources(&mut test, owner, 200);
    let (read_after, withdraw_after) =
        read_and_withdraw_points(&mut test, owner, &owner_proof, &owner_secret, recipient);

    // Every call decodes the account's vault map, so both grow with it. A withdrawal leaves that map unchanged and
    // must not also pay to re-encode it.
    let read_growth = read_after - read_before;
    let withdraw_growth = withdraw_after - withdraw_before;
    assert!(
        withdraw_growth <= read_growth,
        "withdraw grew by {withdraw_growth} points, a read by {read_growth}"
    );
}

#[test]
fn third_party_deposits_cannot_grow_an_account_past_the_vault_limit() {
    let mut test = TemplateTest::new(CRATE_PATH, [NEW_RESOURCE_DEPOSITOR]);
    let depositor = test.get_template_address("NewResourceDepositor");

    let (owner, owner_proof, owner_secret) = test.create_funded_account();
    let (recipient, _, _) = test.create_empty_account();
    let initial_vaults = u32::try_from(account_vault_count(&mut test, owner)).unwrap();
    deposit_new_resources(&mut test, owner, MAX_ACCOUNT_VAULTS - initial_vaults);
    assert_eq!(account_vault_count(&mut test, owner), MAX_ACCOUNT_VAULTS as usize);

    let reason = test.execute_expect_failure(
        test.transaction()
            .call_function(depositor, "deposit_new_resources", args![owner, 1])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(
        reason,
        format!("Account holds the maximum of {MAX_ACCOUNT_VAULTS} vaults"),
    );
    assert_eq!(account_vault_count(&mut test, owner), MAX_ACCOUNT_VAULTS as usize);

    // A full account still pays fees, sends, and receives resources it already holds.
    test.enable_fees();
    test.execute_expect_success(
        test.transaction()
            .pay_fee_from_component(owner, 1_000_000u64)
            .call_method(owner, "withdraw", args![TARI_TOKEN, 1000])
            .put_last_instruction_output_on_workspace("sent")
            .call_method(recipient, "deposit", args![Workspace("sent")])
            .call_method(owner, "withdraw", args![TARI_TOKEN, 10])
            .put_last_instruction_output_on_workspace("kept")
            .call_method(owner, "deposit", args![Workspace("kept")])
            .finish()
            .seal(&owner_secret),
        vec![owner_proof],
    );
}
