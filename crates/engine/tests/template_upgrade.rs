//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_engine::{
    runtime::{NativeAction, RuntimeError},
    transaction::TransactionErrorKind,
};
use tari_engine_types::{
    commit_result::{ExecutionFailureCode, RejectReason},
    indexed_value::IndexedValue,
};
use tari_ootle_common_types::crypto::create_key_pair_from_seed;
use tari_ootle_transaction::{Epoch, Transaction, args, builder::named_args::NamedArg};
use tari_template_lib::types::{
    AccessRule,
    ComponentAddress,
    NonFungibleAddress,
    OwnerRule,
    TemplateAddress,
    VaultId,
    access_rules::UpdateRule,
    crypto::RistrettoPublicKeyBytes,
    rule,
};
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

fn setup() -> (TemplateTest, ComponentAddress) {
    let mut test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/template_upgrade/v1",
        "tests/templates/template_upgrade/v2",
    ]);

    let component = create_component(&mut test);
    (test, component)
}

fn create_component(test_mut: &mut TemplateTest) -> ComponentAddress {
    create_component_with(test_mut, "new", args![
        OwnerRule::OwnedBySigner,
        UpdateRule::Owner,
        signers()
    ])
}

fn signers() -> Vec<RistrettoPublicKeyBytes> {
    (0..5)
        .map(create_key_pair_from_seed)
        .map(|(_, pk)| pk.to_byte_type())
        .collect()
}

fn create_component_with(test_mut: &mut TemplateTest, function: &str, args: Vec<NamedArg>) -> ComponentAddress {
    let v1_template = test_mut.get_template_address("TemplateV1");

    test_mut.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .call_function(v1_template, function, args)
            .finish()
            .seal(test_mut.secret_key()),
        vec![],
    );

    test_mut.return_values()[0].decoded::<ComponentAddress>().unwrap()
}

fn template_address_of(test: &TemplateTest, component: ComponentAddress) -> TemplateAddress {
    *test
        .read_only_state_store()
        .get_component(component)
        .unwrap()
        .template_address()
}

#[test]
fn it_sets_migrate_to_true_in_the_function_def() {
    let test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/template_upgrade/v1",
        "tests/templates/template_upgrade/v2",
    ]);
    let t2 = test.get_module("TemplateV2"); // Load v2 module
    let migrate_fn = t2.template_def().get_function("migrate_v1_to_v2").unwrap();
    assert!(migrate_fn.is_migration);
}

#[test]
fn it_migrates_to_a_new_template() {
    let (mut test, component) = setup();
    let v2_template = test.get_template_address("TemplateV2");

    test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(component, v2_template, "migrate_v1_to_v2", args![])
            .call_method(component, "assert_correct", args![])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    let (_, component) = test
        .read_only_state_store()
        .get_components_by_template_address(v2_template)
        .unwrap()
        .remove(0);
    assert_eq!(*component.template_address(), v2_template);
}

#[test]
fn it_migrates_to_a_new_template_with_args() {
    let (mut test, component) = setup();
    let v2_template = test.get_template_address("TemplateV2");

    test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(
                component,
                v2_template,
                "migrate_v1_to_v2_with_args",
                args!["Something new"],
            )
            .call_method(component, "assert_correct", args![])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    let (_, component) = test
        .read_only_state_store()
        .get_components_by_template_address(v2_template)
        .unwrap()
        .remove(0);
    assert_eq!(*component.template_address(), v2_template);

    // TemplateV2 fields: 0=signers, 1=current_id, 2=new_data, 3=manager, 4=supply_vault.
    let new_data = IndexedValue::from_value(component.into_state())
        .unwrap()
        .get_value::<String>("$.2")
        .unwrap()
        .unwrap();

    assert_eq!(new_data, "Something new");
}

#[test]
fn it_denies_migration_if_not_owner() {
    let (mut test, component) = setup();
    let v1_template = test.get_template_address("TemplateV1");
    let v2_template = test.get_template_address("TemplateV2");

    let (secret, _) = create_key_pair_from_seed(12);

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(
                component,
                v2_template,
                "migrate_v1_to_v2_with_args",
                args!["Something new"],
            )
            .call_method(component, "assert_correct", args![])
            .finish()
            .seal(&secret),
        vec![],
    );

    assert_reject_reason(reason, RuntimeError::AccessDeniedOwnerRequired {
        action: NativeAction::UpdateComponentTemplate.into(),
    });

    let (_, component) = test
        .read_only_state_store()
        .get_components_by_template_address(v1_template)
        .unwrap()
        .remove(0);
    assert_eq!(*component.template_address(), v1_template);
}

#[test]
fn it_fails_when_a_migration_drops_a_vault() {
    let (mut test, component) = setup();
    let v1_template = test.get_template_address("TemplateV1");
    let v2_template = test.get_template_address("TemplateV2");

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(
                component,
                v2_template,
                "faulty_migrate_drop_vault",
                args![],
            )
            .call_method(component, "assert_correct", args![])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
    let (_, component) = test
        .read_only_state_store()
        .get_components_by_template_address(v1_template)
        .unwrap()
        .remove(0);

    // The migration aborts, so the component still has the V1 layout: 0=signers, 1=manager, 2=supply_vault.
    let vault_id = IndexedValue::from_value(component.into_state())
        .unwrap()
        .get_value::<VaultId>("$.2")
        .unwrap()
        .unwrap();

    assert_reject_reason(reason, RuntimeError::OrphanedSubstate { id: vault_id.into() });
}

#[test]
fn it_fails_when_a_migration_panics() {
    let (mut test, component) = setup();
    let v2_template = test.get_template_address("TemplateV2");

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(component, v2_template, "faulty_migrate_panic", args![])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, RejectReason::ExecutionFailure {
        code: ExecutionFailureCode::TemplateError,
        message: "At instruction #1: Template error: Intentional panic during migration".to_string(),
    });
}

#[test]
fn it_migrates_to_a_new_template_without_migration_call() {
    // Migrates a component to the new template without calling the migration function
    // The call to assert_correct works because the v2 component (with the v1_compat feature) is compatible with the v1
    // state It's worth noting that the call to update_component_template_address is not able to perform any state
    // validation, so it's possible to upgrade to an incompatible template making the component unusable. However,
    // the template address could be reverted back to a compatible template to recover the component.

    let mut test = TemplateTest::new(CRATE_PATH, vec![
        ("tests/templates/template_upgrade/v1", &[] as &[&str]),
        ("tests/templates/template_upgrade/v2", &["v1_compat"]),
    ]);

    let component = create_component(&mut test);
    let v2_template = test.get_template_address("TemplateV2");

    test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template(component, v2_template)
            .call_method(component, "assert_correct", args![])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    let (_, component) = test
        .read_only_state_store()
        .get_components_by_template_address(v2_template)
        .unwrap()
        .remove(0);
    assert_eq!(*component.template_address(), v2_template);
}

#[test]
fn it_fails_when_a_migration_attempts_a_cross_template_call() {
    let (mut test, component) = setup();
    let v2_template = test.get_template_address("TemplateV2");

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(
                component,
                v2_template,
                "faulty_migrate_cross_template_call",
                args![],
            )
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(
        reason,
        "Host operation 'call_invoke' is forbidden inside a component migration",
    );
}

/// A migration frame reformats its component's own state and nothing else: it runs with the same restrictions as a
/// resource auth hook, so it cannot create a vault, move funds, hold a proof or allocate an address.
#[test]
fn it_refuses_every_action_but_a_state_rewrite_in_a_migration() {
    let (mut test, component) = setup();
    let v2_template = test.get_template_address("TemplateV2");

    for (function, expected) in [
        ("faulty_migrate_create_vault", "Write to vault_"),
        ("faulty_migrate_withdraw", "Write to vault_"),
        ("faulty_migrate_create_proof", "Write to vault_"),
        (
            "faulty_migrate_allocate_address",
            "Host operation 'allocate_address_invoke'",
        ),
    ] {
        let reason = test.execute_expect_failure(
            Transaction::builder_localnet(Epoch(1))
                .update_component_template_address_with_migrate(component, v2_template, function, args![])
                .finish()
                .seal(test.secret_key()),
            vec![],
        );
        let message = reason.to_string();
        assert!(
            message.contains(expected) && message.contains("a component migration"),
            "{function} gave: {reason}"
        );
    }
}

/// The upgrade rule is evaluated before any migration code runs.
#[test]
fn it_checks_the_upgrade_rule_before_running_the_migration() {
    let mut test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/template_upgrade/v1",
        "tests/templates/template_upgrade/v2",
    ]);
    let v2_template = test.get_template_address("TemplateV2");
    let component = create_component_with(&mut test, "new", args![
        OwnerRule::OwnedBySigner,
        UpdateRule::AccessRule(AccessRule::DenyAll),
        signers()
    ]);

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(component, v2_template, "faulty_migrate_panic", args![])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(reason, RuntimeError::AccessDenied {
        action_ident: NativeAction::UpdateComponentTemplate.into(),
    });
}

#[test]
fn it_disallows_calling_the_migration_function_directly() {
    let mut test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/template_upgrade/v1",
        "tests/templates/template_upgrade/v2",
    ]);
    let v2_template = test.get_template_address("TemplateV2");

    let (secret, _) = create_key_pair_from_seed(12);

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .call_function(v2_template, "migrate_v1_to_v2", args![])
            .finish()
            .seal(&secret),
        vec![],
    );

    assert_reject_reason(reason, TransactionErrorKind::CannotCallMigrationFunctionDirectly {
        name: "migrate_v1_to_v2".to_string(),
    });
}

#[test]
fn it_refuses_to_upgrade_a_component_created_with_the_default_upgrade_rule() {
    let mut test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/template_upgrade/v1",
        "tests/templates/template_upgrade/v2",
    ]);
    let component = create_component_with(&mut test, "new_with_default_upgrade_rule", args![
        OwnerRule::OwnedBySigner,
        signers()
    ]);
    let v1_template = test.get_template_address("TemplateV1");
    let v2_template = test.get_template_address("TemplateV2");

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(component, v2_template, "migrate_v1_to_v2", args![])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, RuntimeError::ComponentUpgradesLocked { component });
    assert_eq!(template_address_of(&test, component), v1_template);
}

#[test]
fn it_refuses_to_upgrade_a_locked_component_even_for_its_owner() {
    let mut test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/template_upgrade/v1",
        "tests/templates/template_upgrade/v2",
    ]);
    let component = create_component_with(&mut test, "new", args![
        OwnerRule::OwnedBySigner,
        UpdateRule::Locked,
        signers()
    ]);
    let v2_template = test.get_template_address("TemplateV2");

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(component, v2_template, "migrate_v1_to_v2", args![])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, RuntimeError::ComponentUpgradesLocked { component });
}

#[test]
fn it_refuses_an_owner_upgrade_rule_on_an_ownerless_component() {
    let mut test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/template_upgrade/v1",
        "tests/templates/template_upgrade/v2",
    ]);
    let component = create_component_with(&mut test, "new", args![OwnerRule::None, UpdateRule::Owner, signers()]);
    let v2_template = test.get_template_address("TemplateV2");

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(component, v2_template, "migrate_v1_to_v2", args![])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, RuntimeError::AccessDeniedOwnerRequired {
        action: NativeAction::UpdateComponentTemplate.into(),
    });
}

#[test]
fn it_authorizes_an_access_rule_upgrade_rule_independently_of_the_owner() {
    let mut test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/template_upgrade/v1",
        "tests/templates/template_upgrade/v2",
    ]);
    let v2_template = test.get_template_address("TemplateV2");
    let (upgrader_secret, upgrader_pk) = create_key_pair_from_seed(12);
    let upgrader_pk = upgrader_pk.to_byte_type();

    // A signer rule lets its key upgrade an ownerless component.
    let open_component = create_component_with(&mut test, "new", args![
        OwnerRule::None,
        UpdateRule::AccessRule(rule!(public_key(upgrader_pk))),
        signers()
    ]);
    test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(open_component, v2_template, "migrate_v1_to_v2", args![])
            .call_method(open_component, "assert_correct", args![])
            .finish()
            .seal(&upgrader_secret),
        vec![],
    );
    assert_eq!(template_address_of(&test, open_component), v2_template);

    // DenyAll refuses the owner.
    let closed_component = create_component_with(&mut test, "new", args![
        OwnerRule::OwnedBySigner,
        UpdateRule::AccessRule(AccessRule::DenyAll),
        signers()
    ]);
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet(Epoch(1))
            .update_component_template_address_with_migrate(closed_component, v2_template, "migrate_v1_to_v2", args![])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(reason, RuntimeError::AccessDenied {
        action_ident: NativeAction::UpdateComponentTemplate.into(),
    });
}

#[test]
fn it_rejects_invalid_upgrade_rules_at_creation() {
    let mut test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/template_upgrade/v1",
        "tests/templates/template_upgrade/v2",
    ]);
    let some_component = create_component(&mut test);
    let v1_template = test.get_template_address("TemplateV1");
    let v2_template = test.get_template_address("TemplateV2");
    let signer_badge = NonFungibleAddress::from_public_key(test.to_public_key_bytes());

    let scoped_reason = || RuntimeError::InvalidArgument {
        argument: "upgrade_rule",
        reason: "component(..)/template(..) cannot be used in a component upgrade rule".to_string(),
    };
    let cases = [
        (rule!(component(some_component)), scoped_reason()),
        (rule!(template(v2_template)), scoped_reason()),
        (
            rule!(m_of_n(2, non_fungible(signer_badge))),
            RuntimeError::InvalidMOfNThreshold {
                argument: "upgrade_rule",
                threshold: 2,
                num_requirements: 1,
            },
        ),
    ];

    for (upgrade_rule, expected) in cases {
        let reason = test.execute_expect_failure(
            Transaction::builder_localnet(Epoch(1))
                .call_function(v1_template, "new", args![
                    OwnerRule::OwnedBySigner,
                    UpdateRule::AccessRule(upgrade_rule),
                    signers()
                ])
                .finish()
                .seal(test.secret_key()),
            vec![],
        );

        assert_reject_reason(reason, expected);
    }
}
