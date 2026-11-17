//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_engine::{runtime::RuntimeError, transaction::TransactionErrorKind};
use tari_ootle_common_types::crypto::create_key_pair_from_seed;
use tari_ootle_transaction::{
    ComponentReference,
    Epoch,
    Instruction,
    MigrateFunction,
    Transaction,
    args,
    builder::named_args::NamedArg,
};
use tari_template_lib::types::{
    AccessRule,
    ComponentAddress,
    NonFungibleAddress,
    OwnerRule,
    TemplateAddress,
    access_rules::{RestrictedAccessRule, UpdateRule},
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
fn it_rejects_a_component_template_update() {
    let (mut test, component) = setup();
    let v1_template = test.get_template_address("TemplateV1");
    let v2_template = test.get_template_address("TemplateV2");

    let migrations = [
        None,
        Some(MigrateFunction {
            name: "migrate_v1_to_v2".try_into().unwrap(),
            args: vec![],
        }),
    ];
    for migrate in migrations {
        let reason = test.execute_expect_failure(
            Transaction::builder_localnet(Epoch(1))
                .add_instruction(Instruction::UpdateComponentTemplate {
                    component: ComponentReference::Address(component),
                    migrate,
                    new_template: v2_template,
                })
                .finish()
                .seal(test.secret_key()),
            vec![],
        );
        assert_reject_reason(reason, TransactionErrorKind::ComponentTemplateUpdateNotSupported);
        assert_eq!(template_address_of(&test, component), v1_template);
    }
}

#[test]
fn it_sets_migrate_to_true_in_the_function_def() {
    let test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/template_upgrade/v1",
        "tests/templates/template_upgrade/v2",
    ]);
    let t2 = test.get_module("TemplateV2"); // Load v2 module
    let migrate_fn = t2.template_def().get_function("migrate_v1_to_v2").unwrap();
    assert!(migrate_fn.is_migration());
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
        (
            AccessRule::Restricted(RestrictedAccessRule::AnyOf(Box::new([]))),
            RuntimeError::EmptyAccessRuleComposite {
                argument: "upgrade_rule",
                composite: "any_of",
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
