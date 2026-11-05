//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_engine::runtime::{ActionIdent, NativeAction, RuntimeError};
use tari_engine_types::commit_result::ExecutionFailureCode;
use tari_ootle_transaction::{Epoch, Transaction, args, args::WorkspaceOffsetId};
use tari_template_lib::{
    invoke_args,
    models::ProofId,
    types::{
        AccessRule,
        Amount,
        ComponentAddress,
        NonFungibleAddress,
        NonFungibleId,
        OwnerRule,
        ResourceAddress,
        VaultId,
        access_rules::{ComponentAccessRules, ResourceAccessRules, ResourceAuthAction, UpdateRule},
        constants::TARI_TOKEN,
        crypto::RistrettoPublicKeyBytes,
        rule,
    },
};
use tari_template_test_tooling::{
    TemplateTest,
    support::assert_error::{assert_access_denied_for_action, assert_reject_reason, assert_reject_reason_with_code},
};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

/// A `ProofWindow` component holding a badge and tokens that only the badge may withdraw, plus the badge's address.
fn proof_window_component(test: &mut TemplateTest) -> (ComponentAddress, NonFungibleAddress) {
    let template = test.get_template_address("ProofWindow");
    test.execute_expect_success(
        test.transaction()
            .call_function(template, "new", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    let component = test.return_values()[0].decoded::<ComponentAddress>().unwrap();
    let badge: NonFungibleAddress = test.call_method(component, "badge", args![], vec![]);
    (component, badge)
}

/// A second `ProofWindow` component whose tokens are gated on `badge_resource`, a badge it does not hold.
fn gated_component(test: &mut TemplateTest, badge_resource: ResourceAddress) -> ComponentAddress {
    let template = test.get_template_address("ProofWindow");
    test.execute_expect_success(
        test.transaction()
            .call_function(template, "new_gated_by", args![badge_resource])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    test.return_values()[0].decoded::<ComponentAddress>().unwrap()
}

#[test]
fn it_opens_and_closes_an_authorization_window() {
    let mut test = TemplateTest::new(CRATE_PATH, Vec::<&str>::new());
    let (account, owner_proof, owner_key) = test.create_funded_account();

    test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .call_method(account, "create_proof_for_resource", args![TARI_TOKEN])
            .put_last_instruction_output_on_workspace("proof")
            .with_authorized_proof("proof", |builder| builder)
            .drop_all_proofs_in_workspace()
            .build_and_seal(&owner_key),
        vec![owner_proof],
    );
}

#[test]
fn it_rejects_dropping_an_authorization_that_is_not_open() {
    let mut test = TemplateTest::new(CRATE_PATH, Vec::<&str>::new());
    let (account, owner_proof, owner_key) = test.create_funded_account();

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .call_method(account, "create_proof_for_resource", args![TARI_TOKEN])
            .put_last_instruction_output_on_workspace("proof")
            .with_authorized_proof("proof", |builder| builder)
            .drop_proof_authorization("proof")
            .drop_all_proofs_in_workspace()
            .build_and_seal(&owner_key),
        vec![owner_proof],
    );

    assert_reject_reason_with_code(
        reason,
        RuntimeError::ProofNotAuthorized {
            proof_id: ProofId::from(0),
        },
        ExecutionFailureCode::InvalidArgument,
    );
}

#[test]
fn it_rejects_authorizing_a_proof_whose_window_is_already_open() {
    let mut test = TemplateTest::new(CRATE_PATH, Vec::<&str>::new());
    let (account, owner_proof, owner_key) = test.create_funded_account();

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .call_method(account, "create_proof_for_resource", args![TARI_TOKEN])
            .put_last_instruction_output_on_workspace("proof")
            .authorize_proof("proof")
            .authorize_proof("proof")
            .drop_all_proofs_in_workspace()
            .build_and_seal(&owner_key),
        vec![owner_proof],
    );

    assert_reject_reason_with_code(
        reason,
        RuntimeError::ProofAlreadyAuthorized {
            proof_id: ProofId::from(0),
        },
        ExecutionFailureCode::InvalidArgument,
    );
}

#[test]
fn a_template_cannot_open_a_second_window_over_the_same_proof() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/proof_window"]);
    let (component, _) = proof_window_component(&mut test);

    test.execute_expect_success(
        test.transaction()
            .call_method(component, "try_nested_window", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    assert!(!test.return_values()[0].decoded::<bool>().unwrap());
}

#[test]
fn it_rejects_authorizing_a_workspace_item_that_is_not_a_proof() {
    let mut test = TemplateTest::new(CRATE_PATH, Vec::<&str>::new());
    let (account, owner_proof, owner_key) = test.create_funded_account();

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .call_method(account, "withdraw", args![TARI_TOKEN, 10])
            .put_last_instruction_output_on_workspace("bucket")
            .authorize_proof("bucket")
            .call_method(account, "deposit", args![Workspace("bucket")])
            .build_and_seal(&owner_key),
        vec![owner_proof],
    );

    assert_reject_reason_with_code(
        reason,
        RuntimeError::WorkspaceItemNotAProof {
            key: WorkspaceOffsetId::new(0),
        },
        ExecutionFailureCode::InvalidArgument,
    );
}

#[test]
fn it_rejects_dropping_the_authorization_of_a_workspace_item_that_is_not_a_proof() {
    let mut test = TemplateTest::new(CRATE_PATH, Vec::<&str>::new());
    let (account, owner_proof, owner_key) = test.create_funded_account();

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .call_method(account, "withdraw", args![TARI_TOKEN, 10])
            .put_last_instruction_output_on_workspace("bucket")
            .drop_proof_authorization("bucket")
            .call_method(account, "deposit", args![Workspace("bucket")])
            .build_and_seal(&owner_key),
        vec![owner_proof],
    );

    assert_reject_reason_with_code(
        reason,
        RuntimeError::WorkspaceItemNotAProof {
            key: WorkspaceOffsetId::new(0),
        },
        ExecutionFailureCode::InvalidArgument,
    );
}

#[test]
fn it_rejects_authorizing_a_dropped_proof() {
    let mut test = TemplateTest::new(CRATE_PATH, Vec::<&str>::new());
    let (account, owner_proof, owner_key) = test.create_funded_account();

    // Dropping the workspace's proofs leaves the item at "proof" naming a proof that no longer exists.
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .call_method(account, "create_proof_for_resource", args![TARI_TOKEN])
            .put_last_instruction_output_on_workspace("proof")
            .drop_all_proofs_in_workspace()
            .authorize_proof("proof")
            .build_and_seal(&owner_key),
        vec![owner_proof],
    );

    assert_reject_reason_with_code(
        reason,
        RuntimeError::ProofNotInScope {
            proof_id: ProofId::from(0),
        },
        ExecutionFailureCode::InvalidArgument,
    );
}

#[test]
fn it_revokes_method_access_when_the_authorization_is_dropped() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/access_rules"]);
    let (account, owner_proof, owner_key) = test.create_funded_account();
    let access_rules_template = test.get_template_address("AccessRulesTest");

    test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .call_function(access_rules_template, "with_configured_rules", args![
                OwnerRule::None,
                ComponentAccessRules::new()
                    .add_method_rule("set_value", rule!(resource(TARI_TOKEN)))
                    .default(AccessRule::AllowAll),
                ResourceAccessRules::new(),
                AccessRule::DenyAll,
            ])
            .build_and_seal(&owner_key),
        vec![owner_proof.clone()],
    );
    let component_address = test.return_values()[0].decoded::<ComponentAddress>().unwrap();

    test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .call_method(account, "create_proof_for_resource", args![TARI_TOKEN])
            .put_last_instruction_output_on_workspace("proof")
            .with_authorized_proof("proof", |builder| {
                builder.call_method(component_address, "set_value", args![1u32])
            })
            .drop_all_proofs_in_workspace()
            .build_and_seal(&owner_key),
        vec![owner_proof.clone()],
    );

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .call_method(account, "create_proof_for_resource", args![TARI_TOKEN])
            .put_last_instruction_output_on_workspace("proof")
            .with_authorized_proof("proof", |builder| builder)
            .call_method(component_address, "set_value", args![2u32])
            .drop_all_proofs_in_workspace()
            .build_and_seal(&owner_key),
        vec![owner_proof],
    );

    assert_access_denied_for_action(reason, ActionIdent::ComponentCallMethod {
        component_address,
        method: "set_value".to_string(),
    });
}

#[test]
fn a_workspace_proof_authorizes_nothing_until_its_window_is_opened() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/proof_window"]);
    let (account, _, _) = test.create_empty_account();
    let (component, _) = proof_window_component(&mut test);

    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(component, "create_badge_proof", args![])
            .put_last_instruction_output_on_workspace("proof")
            .call_method(component, "take_tokens", args![10])
            .put_last_instruction_output_on_workspace("tokens")
            .call_method(account, "deposit", args![Workspace("tokens")])
            .drop_all_proofs_in_workspace()
            .build_and_seal(test.secret_key()),
        vec![],
    );
    assert_access_denied_for_action(reason, ResourceAuthAction::Withdraw);

    test.execute_expect_success(
        test.transaction()
            .call_method(component, "create_badge_proof", args![])
            .put_last_instruction_output_on_workspace("proof")
            .with_authorized_proof("proof", |builder| {
                builder
                    .call_method(component, "take_tokens", args![10])
                    .put_last_instruction_output_on_workspace("tokens")
                    .call_method(account, "deposit", args![Workspace("tokens")])
            })
            .drop_all_proofs_in_workspace()
            .build_and_seal(test.secret_key()),
        vec![],
    );
}

#[test]
fn a_held_proof_authorizes_nothing_in_a_template() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/proof_window"]);
    let (account, _, _) = test.create_empty_account();
    let (component, _) = proof_window_component(&mut test);

    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(component, "take_tokens_holding_proof", args![10])
            .put_last_instruction_output_on_workspace("tokens")
            .call_method(account, "deposit", args![Workspace("tokens")])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    assert_access_denied_for_action(reason, ResourceAuthAction::Withdraw);

    test.execute_expect_success(
        test.transaction()
            .call_method(component, "take_tokens_in_window", args![10])
            .put_last_instruction_output_on_workspace("tokens")
            .call_method(account, "deposit", args![Workspace("tokens")])
            .build_and_seal(test.secret_key()),
        vec![],
    );
}

#[test]
fn a_window_lends_the_proof_to_the_callee_and_no_further() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/proof_window"]);
    let (account, _, _) = test.create_empty_account();
    let (lender, badge) = proof_window_component(&mut test);
    let gate = gated_component(&mut test, *badge.resource_address());
    let relay = gated_component(&mut test, *badge.resource_address());

    // Holding the proof across the call lends nothing.
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(lender, "call_holding_badge", args![gate, "take_tokens", invoke_args![
                Amount::from(10u64)
            ]])
            .put_last_instruction_output_on_workspace("tokens")
            .call_method(account, "deposit", args![Workspace("tokens")])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    assert_access_denied_for_action(reason, ResourceAuthAction::Withdraw);

    // A window around the call lends the proof to the callee's frame.
    test.execute_expect_success(
        test.transaction()
            .call_method(lender, "lend_badge_to", args![gate, "take_tokens", invoke_args![
                Amount::from(10u64)
            ]])
            .put_last_instruction_output_on_workspace("tokens")
            .call_method(account, "deposit", args![Workspace("tokens")])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    // The callee is lent the proof but does not hold it, so its own calls are lent nothing.
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(lender, "lend_badge_to", args![relay, "relay", invoke_args![
                gate,
                "take_tokens",
                invoke_args![Amount::from(10u64)]
            ]])
            .put_last_instruction_output_on_workspace("tokens")
            .call_method(account, "deposit", args![Workspace("tokens")])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    assert_access_denied_for_action(reason, ResourceAuthAction::Withdraw);

    // The same one hop from a transaction's window: the relay is lent the proof, the gate is not.
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(lender, "create_badge_proof", args![])
            .put_last_instruction_output_on_workspace("proof")
            .with_authorized_proof("proof", |builder| {
                builder
                    .call_method(relay, "relay", args![gate, "take_tokens", invoke_args![Amount::from(
                        10u64
                    )]])
                    .put_last_instruction_output_on_workspace("tokens")
                    .call_method(account, "deposit", args![Workspace("tokens")])
            })
            .drop_all_proofs_in_workspace()
            .build_and_seal(test.secret_key()),
        vec![],
    );
    assert_access_denied_for_action(reason, ResourceAuthAction::Withdraw);
}

#[test]
fn a_lent_proof_satisfies_the_callees_method_rule_and_badge_assertion() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/proof_window"]);
    let (lender, badge) = proof_window_component(&mut test);

    // A held proof carries no badge into the call.
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(lender, "create_badge_proof", args![])
            .put_last_instruction_output_on_workspace("proof")
            .call_method(lender, "assert_badge_in_scope", args![])
            .drop_all_proofs_in_workspace()
            .build_and_seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(reason, RuntimeError::BadgeNotInScope { badge: badge.clone() });

    test.execute_expect_success(
        test.transaction()
            .call_method(lender, "create_badge_proof", args![])
            .put_last_instruction_output_on_workspace("proof")
            .with_authorized_proof("proof", |builder| {
                builder.call_method(lender, "assert_badge_in_scope", args![])
            })
            .drop_all_proofs_in_workspace()
            .build_and_seal(test.secret_key()),
        vec![],
    );

    // A template's window lends the badge to the callee's assertion too.
    test.execute_expect_success(
        test.transaction()
            .call_method(lender, "lend_badge_to", args![
                lender,
                "assert_badge_in_scope",
                invoke_args![]
            ])
            .build_and_seal(test.secret_key()),
        vec![],
    );
}

#[test]
fn a_shown_proof_can_be_read_and_nothing_else() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/proof_window"]);
    let (component, badge) = proof_window_component(&mut test);
    let other = gated_component(&mut test, *badge.resource_address());

    test.execute_expect_success(
        test.transaction()
            .call_method(component, "create_badge_proof", args![])
            .put_last_instruction_output_on_workspace("proof")
            .call_method(other, "inspect", args![Workspace("proof")])
            .call_method(other, "try_authorize_shown", args![Workspace("proof")])
            .drop_all_proofs_in_workspace()
            .build_and_seal(test.secret_key()),
        vec![],
    );
    let (resource, amount, ids) = test.return_values()[2]
        .decoded::<(ResourceAddress, Amount, Vec<NonFungibleId>)>()
        .unwrap();
    assert_eq!(resource, *badge.resource_address());
    assert_eq!(amount, Amount::from(1u64));
    assert_eq!(ids, vec![NonFungibleId::from_string("badge")]);
    assert!(!test.return_values()[3].decoded::<bool>().unwrap());

    // The shown proof does not carry its badge into the frame it is shown to.
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(component, "create_badge_proof", args![])
            .put_last_instruction_output_on_workspace("proof")
            .call_method(other, "assert_badge_in_scope_shown", args![Workspace("proof")])
            .drop_all_proofs_in_workspace()
            .build_and_seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(reason, RuntimeError::BadgeNotInScope { badge });

    for method in ["pass_on_shown", "return_shown", "drop_shown"] {
        let args = if method == "pass_on_shown" {
            args![component, Workspace("proof")]
        } else {
            args![Workspace("proof")]
        };
        let reason = test.execute_expect_failure(
            test.transaction()
                .call_method(component, "create_badge_proof", args![])
                .put_last_instruction_output_on_workspace("proof")
                .call_method(other, method, args)
                .drop_all_proofs_in_workspace()
                .build_and_seal(test.secret_key()),
            vec![],
        );
        assert_reject_reason(reason, RuntimeError::ProofNotHeld {
            proof_id: ProofId::from(0),
        });
    }
}

/// A fee intent must drop every proof it created before it ends, so no window is open at the fee boundary and
/// the main instructions start with none.
#[test]
fn the_fee_boundary_closes_every_window() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/proof_window"]);
    let (account, owner_proof, owner_key) = test.create_funded_account();
    let (component, _) = proof_window_component(&mut test);

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .with_fee_instructions_builder(|builder| {
                builder
                    .pay_fee_from_component(account, Amount::from(1000u64))
                    .call_method(component, "create_badge_proof", args![])
                    .put_last_instruction_output_on_workspace("proof")
                    .authorize_proof("proof")
            })
            .call_method(component, "take_tokens", args![10])
            .put_last_instruction_output_on_workspace("tokens")
            .call_method(account, "deposit", args![Workspace("tokens")])
            .build_and_seal(&owner_key),
        vec![owner_proof.clone()],
    );
    assert_reject_reason(reason, "1 dangling proofs remain after transaction execution");

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .with_fee_instructions_builder(|builder| {
                builder
                    .pay_fee_from_component(account, Amount::from(1000u64))
                    .call_method(component, "create_badge_proof", args![])
                    .put_last_instruction_output_on_workspace("proof")
                    .authorize_proof("proof")
                    .drop_all_proofs_in_workspace()
            })
            .call_method(component, "take_tokens", args![10])
            .put_last_instruction_output_on_workspace("tokens")
            .call_method(account, "deposit", args![Workspace("tokens")])
            .build_and_seal(&owner_key),
        vec![owner_proof],
    );
    assert_access_denied_for_action(reason, ResourceAuthAction::Withdraw);
}

#[test]
fn a_resource_auth_hook_is_lent_nothing() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/access_rules"]);
    let (account, owner_proof, owner_key) = test.create_funded_account();
    let access_rules_template = test.get_template_address("AccessRulesTest");

    test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .call_function(access_rules_template, "with_auth_hook", args![
                true,
                "hook_asserts_badge"
            ])
            .build_and_seal(&owner_key),
        vec![owner_proof.clone()],
    );
    let component = test.return_values()[0].decoded::<ComponentAddress>().unwrap();
    let badge_vault = test.extract_component_value::<VaultId>(component, "$.2");
    let badge_resource = *test
        .read_only_state_store()
        .get_vault(&badge_vault)
        .unwrap()
        .resource_address();

    // The transaction holds the badge the hook asserts, and authorizes it around the withdraw. The acting frame is
    // lent it; the hook's frame is not.
    test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .call_method(component, "take_badge_by_name", args!["mint"])
            .put_last_instruction_output_on_workspace("badge")
            .call_method(account, "deposit", args![Workspace("badge")])
            .build_and_seal(&owner_key),
        vec![owner_proof.clone()],
    );
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .call_method(account, "create_proof_by_non_fungible", args![NonFungibleAddress::new(
                badge_resource,
                NonFungibleId::from_string("mint")
            )])
            .put_last_instruction_output_on_workspace("proof")
            .with_authorized_proof("proof", |builder| {
                builder
                    .call_method(component, "take_tokens", args![10])
                    .put_last_instruction_output_on_workspace("tokens")
                    .call_method(account, "deposit", args![Workspace("tokens")])
            })
            .drop_all_proofs_in_workspace()
            .build_and_seal(&owner_key),
        vec![owner_proof],
    );
    assert_reject_reason(reason, "This call does not carry the badge");
}

#[test]
fn the_upgrade_rule_is_checked_in_the_instructions_scope_and_the_migration_frame_is_lent_nothing() {
    let mut test = TemplateTest::new(CRATE_PATH, [
        "tests/templates/proof_window",
        "tests/templates/template_upgrade/v1",
        "tests/templates/template_upgrade/v2",
    ]);
    let (lender, badge) = proof_window_component(&mut test);
    let v1_template = test.get_template_address("TemplateV1");
    let v2_template = test.get_template_address("TemplateV2");
    let signer_badge = NonFungibleAddress::from_public_key(test.public_key().to_byte_type());

    let create = |test: &mut TemplateTest, upgrade_rule: UpdateRule| {
        test.execute_expect_success(
            test.transaction()
                .call_function(v1_template, "new", args![
                    OwnerRule::OwnedBySigner,
                    upgrade_rule,
                    Vec::<RistrettoPublicKeyBytes>::new()
                ])
                .build_and_seal(test.secret_key()),
            vec![],
        );
        test.return_values()[0].decoded::<ComponentAddress>().unwrap()
    };

    // A rule naming the badge is satisfied by a window the transaction opens, and by nothing less.
    let component = create(
        &mut test,
        UpdateRule::AccessRule(rule!(resource(*badge.resource_address()))),
    );
    let reason = test.execute_expect_failure(
        test.transaction()
            .call_method(lender, "create_badge_proof", args![])
            .put_last_instruction_output_on_workspace("proof")
            .update_component_template_address_with_migrate(component, v2_template, "migrate_v1_to_v2", args![])
            .drop_all_proofs_in_workspace()
            .build_and_seal(test.secret_key()),
        vec![],
    );
    assert_access_denied_for_action(reason, NativeAction::UpdateComponentTemplate);

    test.execute_expect_success(
        test.transaction()
            .call_method(lender, "create_badge_proof", args![])
            .put_last_instruction_output_on_workspace("proof")
            .with_authorized_proof("proof", |builder| {
                builder.update_component_template_address_with_migrate(
                    component,
                    v2_template,
                    "migrate_v1_to_v2",
                    args![],
                )
            })
            .drop_all_proofs_in_workspace()
            .build_and_seal(test.secret_key()),
        vec![],
    );

    // The migration frame itself is lent neither the window's proof nor the signer badge.
    for (expected_badge, wrap_in_window) in [(badge.clone(), true), (signer_badge, false)] {
        let component = create(&mut test, UpdateRule::Owner);
        let reason = test.execute_expect_failure(
            test.transaction()
                .call_method(lender, "create_badge_proof", args![])
                .put_last_instruction_output_on_workspace("proof")
                .then(|builder| {
                    if wrap_in_window {
                        builder.authorize_proof("proof")
                    } else {
                        builder
                    }
                })
                .update_component_template_address_with_migrate(
                    component,
                    v2_template,
                    "migrate_asserting_badge",
                    args![expected_badge.clone()],
                )
                .drop_all_proofs_in_workspace()
                .build_and_seal(test.secret_key()),
            vec![],
        );
        assert_reject_reason(reason, RuntimeError::BadgeNotInScope { badge: expected_badge });
    }
}
