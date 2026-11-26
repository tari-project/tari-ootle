//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey};
use tari_engine::runtime::{ActionIdent, RuntimeError};
use tari_engine_types::{
    commit_result::{ExecutionFailureCode, RejectReason},
    component::Component,
    substate::SubstateId,
};
use tari_ootle_transaction::{Epoch, Transaction, TransactionBuilder, args};
use tari_template_lib::{
    args::ComponentAction,
    types::{
        AccessRule,
        ComponentAddress,
        NonFungibleAddress,
        OwnerRule,
        SubstateOwnerRule,
        access_rules::{ComponentAccessRules, RulesUpdater},
        rule,
    },
};
use tari_template_test_tooling::{
    TemplateTest,
    support::assert_error::{assert_access_denied_for_action, assert_reject_reason, assert_reject_reason_with_code},
};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

struct Signer {
    proof: NonFungibleAddress,
    public_key: RistrettoPublicKey,
    secret_key: RistrettoSecretKey,
}

impl Signer {
    fn new(test: &mut TemplateTest) -> Self {
        let (proof, public_key, secret_key) = test.create_owner_proof();
        Self {
            proof,
            public_key,
            secret_key,
        }
    }

    fn owner_rule(&self) -> OwnerRule {
        OwnerRule::ByPublicKey(self.public_key.to_byte_type())
    }

    fn substate_owner_rule(&self) -> SubstateOwnerRule {
        SubstateOwnerRule::ByPublicKey(self.public_key.to_byte_type())
    }

    fn rule(&self) -> AccessRule {
        rule!(non_fungible(self.proof.clone()))
    }
}

fn setup() -> (TemplateTest, Signer, Signer) {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/rules_updater"]);
    let owner = Signer::new(&mut test);
    let stranger = Signer::new(&mut test);
    (test, owner, stranger)
}

fn tx() -> TransactionBuilder {
    Transaction::builder_localnet(Epoch(1))
}

fn create(test: &mut TemplateTest, owner: &Signer, rules_updater: RulesUpdater) -> ComponentAddress {
    let template = test.get_template_address("RulesUpdaterTest");
    test.execute_expect_success(
        tx().call_function(template, "new", args![owner.owner_rule(), rules_updater])
            .build_and_seal(&owner.secret_key),
        vec![owner.proof.clone()],
    );
    test.return_values()[0].decoded::<ComponentAddress>().unwrap()
}

fn component(test: &TemplateTest, address: ComponentAddress) -> Component {
    test.read_only_state_store()
        .get_substate(&SubstateId::Component(address))
        .unwrap()
        .substate_value()
        .component()
        .unwrap()
        .clone()
}

/// Opens `guarded` to every caller.
fn open_rules() -> ComponentAccessRules {
    ComponentAccessRules::new().default(AccessRule::AllowAll)
}

fn set_rules_by_instruction(caller: &Signer, component: ComponentAddress) -> Transaction {
    tx().set_component_access_rules(component, open_rules())
        .build_and_seal(&caller.secret_key)
}

fn set_owner_by_instruction(caller: &Signer, component: ComponentAddress, owner_rule: OwnerRule) -> Transaction {
    tx().set_component_owner_rule(component, owner_rule)
        .build_and_seal(&caller.secret_key)
}

fn set_rules_from_template(caller: &Signer, component: ComponentAddress) -> Transaction {
    tx().call_method(component, "set_access_rules_from_template", args![open_rules()])
        .build_and_seal(&caller.secret_key)
}

fn set_owner_from_template(caller: &Signer, component: ComponentAddress, owner_rule: SubstateOwnerRule) -> Transaction {
    tx().call_method(component, "set_owner_rule_from_template", args![owner_rule])
        .build_and_seal(&caller.secret_key)
}

fn call_guarded(caller: &Signer, component: ComponentAddress) -> Transaction {
    tx().call_method(component, "guarded", args![])
        .build_and_seal(&caller.secret_key)
}

fn succeeds(test: &mut TemplateTest, caller: &Signer, transaction: Transaction) {
    test.execute_expect_success(transaction, vec![caller.proof.clone()]);
}

fn fails(test: &mut TemplateTest, caller: &Signer, transaction: Transaction) -> RejectReason {
    test.execute_expect_failure(transaction, vec![caller.proof.clone()])
}

fn assert_locked(reason: RejectReason, component: ComponentAddress) {
    assert_reject_reason_with_code(
        reason,
        RuntimeError::ComponentRulesLocked { component },
        ExecutionFailureCode::AccessDenied,
    );
}

fn assert_owner_required(reason: RejectReason, action: ComponentAction) {
    assert_reject_reason_with_code(
        reason,
        RuntimeError::AccessDeniedOwnerRequired {
            action: ActionIdent::from(action),
        },
        ExecutionFailureCode::AccessDenied,
    );
}

#[test]
fn a_component_built_by_the_template_library_is_locked_by_default() {
    let (mut test, owner, _) = setup();
    let template = test.get_template_address("RulesUpdaterTest");
    test.execute_expect_success(
        tx().call_function(template, "new", args![owner.owner_rule(), RulesUpdater::default()])
            .build_and_seal(&owner.secret_key),
        vec![owner.proof.clone()],
    );
    let address = test.return_values()[0].decoded::<ComponentAddress>().unwrap();
    assert!(matches!(
        component(&test, address).header.rules_updater,
        Some(RulesUpdater::Locked)
    ));
}

#[test]
fn locked_rules_change_neither_by_instruction_nor_from_template_code() {
    let (mut test, owner, _) = setup();
    let address = create(&mut test, &owner, RulesUpdater::Locked);

    for transaction in [
        set_rules_by_instruction(&owner, address),
        set_owner_by_instruction(&owner, address, OwnerRule::None),
        set_rules_from_template(&owner, address),
        set_owner_from_template(&owner, address, SubstateOwnerRule::None),
    ] {
        let reason = fails(&mut test, &owner, transaction);
        assert_locked(reason, address);
    }
}

#[test]
fn template_managed_rules_change_from_template_code_only() {
    let (mut test, owner, stranger) = setup();
    let address = create(&mut test, &owner, RulesUpdater::TemplateManaged);

    for transaction in [
        set_rules_by_instruction(&owner, address),
        set_owner_by_instruction(&owner, address, stranger.owner_rule()),
    ] {
        let reason = fails(&mut test, &owner, transaction);
        assert_reject_reason_with_code(
            reason,
            RuntimeError::ComponentRulesTemplateManaged { component: address },
            ExecutionFailureCode::AccessDenied,
        );
    }

    let reason = fails(&mut test, &stranger, set_rules_from_template(&stranger, address));
    assert_owner_required(reason, ComponentAction::SetAccessRules);

    succeeds(&mut test, &owner, set_rules_from_template(&owner, address));
    succeeds(&mut test, &stranger, call_guarded(&stranger, address));
}

/// A template built before the setting existed sends no rules updater, and must keep changing its rules from its own
/// code as it did.
#[test]
fn a_component_created_without_a_rules_updater_is_template_managed() {
    let (mut test, owner, stranger) = setup();
    let template = test.get_template_address("RulesUpdaterTest");
    test.execute_expect_success(
        tx().call_function(template, "new_without_rules_updater", args![owner.owner_rule()])
            .build_and_seal(&owner.secret_key),
        vec![owner.proof.clone()],
    );
    let address = test.return_values()[0].decoded::<ComponentAddress>().unwrap();
    assert!(matches!(
        component(&test, address).header.rules_updater,
        Some(RulesUpdater::TemplateManaged)
    ));

    let reason = fails(&mut test, &owner, set_rules_by_instruction(&owner, address));
    assert_reject_reason(reason, RuntimeError::ComponentRulesTemplateManaged {
        component: address,
    });

    succeeds(
        &mut test,
        &owner,
        set_owner_from_template(&owner, address, stranger.substate_owner_rule()),
    );
    succeeds(&mut test, &stranger, set_rules_from_template(&stranger, address));
}

#[test]
fn the_owner_changes_owner_updatable_rules_by_instruction() {
    let (mut test, owner, stranger) = setup();
    let address = create(&mut test, &owner, RulesUpdater::Owner);

    let reason = fails(&mut test, &stranger, call_guarded(&stranger, address));
    assert_reject_reason(reason, RuntimeError::AccessDenied {
        action_ident: ActionIdent::ComponentCallMethod {
            component_address: address,
            method: "guarded".to_string(),
        },
    });

    let reason = fails(&mut test, &stranger, set_rules_by_instruction(&stranger, address));
    assert_owner_required(reason, ComponentAction::SetAccessRules);
    let reason = fails(
        &mut test,
        &stranger,
        set_owner_by_instruction(&stranger, address, stranger.owner_rule()),
    );
    assert_owner_required(reason, ComponentAction::SetOwnerRule);

    succeeds(&mut test, &owner, set_rules_by_instruction(&owner, address));
    succeeds(&mut test, &stranger, call_guarded(&stranger, address));
    assert!(
        component(&test, address)
            .access_rules()
            .method_access_rules_iter()
            .next()
            .is_none()
    );
}

#[test]
fn an_owner_rule_handed_over_by_instruction_moves_the_right_to_change_rules() {
    let (mut test, owner, successor) = setup();
    let address = create(&mut test, &owner, RulesUpdater::Owner);

    succeeds(
        &mut test,
        &owner,
        set_owner_by_instruction(&owner, address, successor.owner_rule()),
    );
    assert_eq!(*component(&test, address).owner_rule(), successor.substate_owner_rule());

    let reason = fails(&mut test, &owner, set_rules_by_instruction(&owner, address));
    assert_owner_required(reason, ComponentAction::SetAccessRules);
    succeeds(&mut test, &successor, set_rules_by_instruction(&successor, address));
}

#[test]
fn an_owner_updatable_component_also_changes_its_rules_from_template_code() {
    let (mut test, owner, stranger) = setup();
    let address = create(&mut test, &owner, RulesUpdater::Owner);

    let reason = fails(&mut test, &stranger, set_rules_from_template(&stranger, address));
    assert_owner_required(reason, ComponentAction::SetAccessRules);
    succeeds(&mut test, &owner, set_rules_from_template(&owner, address));
}

#[test]
fn an_access_rule_updater_admits_its_rule_and_not_the_owner() {
    let (mut test, owner, operator) = setup();
    let address = create(&mut test, &owner, RulesUpdater::AccessRule(operator.rule()));

    let reason = fails(&mut test, &owner, set_rules_by_instruction(&owner, address));
    assert_access_denied_for_action(reason, ComponentAction::SetAccessRules);
    let reason = fails(
        &mut test,
        &owner,
        set_owner_from_template(&owner, address, operator.substate_owner_rule()),
    );
    assert_access_denied_for_action(reason, ComponentAction::SetOwnerRule);

    succeeds(&mut test, &operator, set_rules_by_instruction(&operator, address));
    succeeds(
        &mut test,
        &operator,
        set_owner_by_instruction(&operator, address, operator.owner_rule()),
    );
    assert_eq!(*component(&test, address).owner_rule(), operator.substate_owner_rule());
}

#[test]
fn owned_by_signer_resolves_to_the_main_signer() {
    let (mut test, owner, operator) = setup();
    let address = create(&mut test, &owner, RulesUpdater::AccessRule(operator.rule()));

    succeeds(
        &mut test,
        &operator,
        set_owner_by_instruction(&operator, address, OwnerRule::OwnedBySigner),
    );
    assert_eq!(*component(&test, address).owner_rule(), operator.substate_owner_rule());
}

#[test]
fn an_instruction_validates_the_rules_it_sets() {
    let (mut test, owner, _) = setup();
    let address = create(&mut test, &owner, RulesUpdater::Owner);

    let reason = fails(
        &mut test,
        &owner,
        tx().set_component_access_rules(
            address,
            open_rules().add_method_rule("no_such_method", AccessRule::AllowAll),
        )
        .build_and_seal(&owner.secret_key),
    );
    assert_reject_reason(reason, "No method 'no_such_method' found in template");

    let reason = fails(
        &mut test,
        &owner,
        set_owner_by_instruction(&owner, address, OwnerRule::ByAccessRule(rule!(component(address)))),
    );
    assert_reject_reason(
        reason,
        "component(..)/template(..) cannot be used in a component owner rule",
    );
}

#[test]
fn a_scoped_rules_updater_is_rejected_at_creation() {
    let (mut test, owner, _) = setup();
    let template = test.get_template_address("RulesUpdaterTest");
    let reason = test.execute_expect_failure(
        tx().call_function(template, "new", args![
            owner.owner_rule(),
            RulesUpdater::AccessRule(rule!(template(template)))
        ])
        .build_and_seal(&owner.secret_key),
        vec![owner.proof.clone()],
    );
    assert_reject_reason(
        reason,
        "component(..)/template(..) cannot be used in a component rules updater",
    );
}

#[test]
fn a_component_created_earlier_in_the_transaction_takes_an_instruction() {
    let (mut test, owner, stranger) = setup();
    let template = test.get_template_address("RulesUpdaterTest");
    test.execute_expect_success(
        tx().call_function(template, "new", args![owner.owner_rule(), RulesUpdater::Owner])
            .put_last_instruction_output_on_workspace("component")
            .set_component_access_rules("component", open_rules())
            .build_and_seal(&owner.secret_key),
        vec![owner.proof.clone()],
    );
    let address = test.return_values()[0].decoded::<ComponentAddress>().unwrap();
    succeeds(&mut test, &stranger, call_guarded(&stranger, address));
}
