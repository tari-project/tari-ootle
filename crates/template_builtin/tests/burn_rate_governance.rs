// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::{
    component::Component,
    virtual_substate::{VirtualSubstate, VirtualSubstateId},
};
use tari_ootle_transaction::{Transaction, args, builder::named_args::NamedArg};
use tari_template_lib::types::{
    SubstateOwnerRule,
    constants::BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS,
    crypto::RistrettoPublicKeyBytes,
    governance::{BurnRateGovernanceState, council_owner_rule},
};
use tari_template_test_tooling::{
    TemplateTest,
    byte_type::ToByteType,
    crypto::{PublicKey, RistrettoPublicKey, RistrettoSecretKey},
    support::assert_error::assert_reject_reason,
};

const CURRENT_EPOCH: u64 = 10;
/// The earliest epoch a rate set in [`CURRENT_EPOCH`] may activate at, by
/// `MIN_BURN_RATE_ACTIVATION_LEAD_EPOCHS`.
const EARLIEST_ACTIVATION: u64 = CURRENT_EPOCH + 2;

/// A harness with a seated council, and the keys to sign as it.
struct Council {
    test: TemplateTest,
    members: Vec<RistrettoSecretKey>,
}

impl Council {
    /// `size` members owning the component at `threshold`, at [`CURRENT_EPOCH`].
    fn seated(size: u8, threshold: u16) -> Self {
        let mut test = TemplateTest::new_builtin_only();
        test.set_virtual_substate(
            VirtualSubstateId::CurrentEpoch,
            VirtualSubstate::CurrentEpoch(CURRENT_EPOCH),
        );

        let members: Vec<_> = (0..size).map(|seed| test.new_key_pair(seed + 1)).collect();
        let public_keys: Vec<RistrettoPublicKeyBytes> = members.iter().map(|(_, pk)| pk.to_byte_type()).collect();
        test.seat_burn_rate_council(threshold, &public_keys);

        Self {
            test,
            members: members.into_iter().map(|(sk, _)| sk).collect(),
        }
    }

    fn public_key(&self, index: usize) -> RistrettoPublicKeyBytes {
        RistrettoPublicKey::from_secret_key(&self.members[index]).to_byte_type()
    }

    /// A call signed by the members at `signers`. The first of them seals it.
    fn call(&mut self, method: &str, signers: &[usize], call_args: Vec<NamedArg>) -> Transaction {
        let seal_index = signers[0];
        let seal_signer = self.public_key(seal_index);

        let mut unsealed = self
            .test
            .transaction()
            .call_method(BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS, method, call_args)
            .finish();
        for index in signers.iter().filter(|i| **i != seal_index) {
            unsealed = unsealed.add_signer(&seal_signer, &self.members[*index]);
        }
        unsealed.seal(&self.members[seal_index])
    }

    fn set_burn_rate(&mut self, signers: &[usize], rate_bps: u16, activation_epoch: u64) -> Transaction {
        self.call("set_burn_rate", signers, args![rate_bps, activation_epoch])
    }

    fn component(&self) -> Component {
        self.test
            .read_only_state_store()
            .get_component(BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS)
            .unwrap()
    }

    fn state(&self) -> BurnRateGovernanceState {
        tari_bor::from_value(self.component().state()).unwrap()
    }

    fn owner_rule(&self) -> SubstateOwnerRule {
        self.component().owner_rule().clone()
    }
}

#[test]
fn a_quorum_of_the_council_sets_the_rate() {
    let mut council = Council::seated(3, 2);
    let transaction = council.set_burn_rate(&[0, 1], 700, EARLIEST_ACTIVATION);
    council.test.execute_expect_success(transaction, vec![]);

    let state = council.state();
    assert_eq!(state.rate_at(EARLIEST_ACTIVATION), Some(700));
    assert_eq!(state.rate_at(EARLIEST_ACTIVATION - 1), None);
}

/// The owner rule is an m-of-n, so the engine denies the call rather than the template counting
/// signatures.
#[test]
fn fewer_signatures_than_the_threshold_are_denied() {
    let mut council = Council::seated(3, 2);
    let transaction = council.set_burn_rate(&[0], 700, EARLIEST_ACTIVATION);
    let reason = council.test.execute_expect_failure(transaction, vec![]);
    assert_reject_reason(&reason, "Access Denied");

    assert_eq!(council.state().rate_at(EARLIEST_ACTIVATION), None);
}

/// A signature from outside the council does not contribute to the threshold.
#[test]
fn signers_outside_the_council_do_not_meet_the_threshold() {
    let mut council = Council::seated(3, 2);
    let (outsider_secret, _) = council.test.new_key_pair(200);
    let member = council.public_key(0);

    let transaction = council
        .test
        .transaction()
        .call_method(BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS, "set_burn_rate", args![
            700u16,
            EARLIEST_ACTIVATION
        ])
        .finish()
        .add_signer(&member, &outsider_secret)
        .seal(&council.members[0]);

    let reason = council.test.execute_expect_failure(transaction, vec![]);
    assert_reject_reason(&reason, "Access Denied");
}

#[test]
fn a_rate_above_the_ceiling_is_rejected() {
    let mut council = Council::seated(3, 2);
    let transaction = council.set_burn_rate(&[0, 1], 10_001, EARLIEST_ACTIVATION);
    let reason = council.test.execute_expect_failure(transaction, vec![]);
    assert_reject_reason(&reason, "is above the 10000bps ceiling");
}

#[test]
fn an_activation_inside_the_lead_time_is_rejected() {
    let mut council = Council::seated(3, 2);
    let transaction = council.set_burn_rate(&[0, 1], 700, EARLIEST_ACTIVATION - 1);
    let reason = council.test.execute_expect_failure(transaction, vec![]);
    assert_reject_reason(&reason, "the earliest epoch a rate set in epoch");
}

#[test]
fn rescheduling_an_activation_that_has_not_fired_replaces_it() {
    let mut council = Council::seated(3, 2);
    let transaction = council.set_burn_rate(&[0, 1], 700, EARLIEST_ACTIVATION);
    council.test.execute_expect_success(transaction, vec![]);
    let transaction = council.set_burn_rate(&[0, 1], 50, EARLIEST_ACTIVATION);
    council.test.execute_expect_success(transaction, vec![]);

    let state = council.state();
    assert_eq!(state.schedule.len(), 1);
    assert_eq!(state.rate_at(EARLIEST_ACTIVATION), Some(50));
}

#[test]
fn a_network_that_seats_no_council_cannot_move_the_rate() {
    let mut council = Council::seated(3, 2);
    council.test.seat_burn_rate_council(0, &[]);
    assert_eq!(council.owner_rule(), SubstateOwnerRule::None);

    let transaction = council.set_burn_rate(&[0, 1], 700, EARLIEST_ACTIVATION);
    let reason = council.test.execute_expect_failure(transaction, vec![]);
    assert_reject_reason(&reason, "Access Denied");
}

/// Retiring leaves an owner rule nobody satisfies, and the engine gates `SetOwnerRule` on the rule in
/// force, so nothing can seat a council again.
#[test]
fn a_retired_council_leaves_the_rate_with_the_table_for_good() {
    let mut council = Council::seated(3, 2);
    let transaction = council.set_burn_rate(&[0, 1], 700, EARLIEST_ACTIVATION);
    council.test.execute_expect_success(transaction, vec![]);

    let transaction = council.call("retire", &[0, 1], vec![]);
    council.test.execute_expect_success(transaction, vec![]);

    assert_eq!(council.owner_rule(), SubstateOwnerRule::None);
    assert_eq!(council.state().retired_from, Some(EARLIEST_ACTIVATION));

    let transaction = council.set_burn_rate(&[0, 1], 700, EARLIEST_ACTIVATION);
    let reason = council.test.execute_expect_failure(transaction, vec![]);
    assert_reject_reason(&reason, "Access Denied");

    let member = council.public_key(0);
    let transaction = council.call("set_council", &[0, 1], args![1u16, vec![member]]);
    let reason = council.test.execute_expect_failure(transaction, vec![]);
    assert_reject_reason(&reason, "Access Denied");
}

/// The rate of the epoch retirement executes in, and of the one after it, may already have been
/// resolved by some shard group, so retiring holds them where the schedule put them.
#[test]
fn retiring_leaves_the_rate_of_the_current_and_next_epoch_unchanged() {
    let mut council = Council::seated(3, 2);
    let transaction = council.set_burn_rate(&[0, 1], 700, EARLIEST_ACTIVATION);
    council.test.execute_expect_success(transaction, vec![]);

    council.test.set_virtual_substate(
        VirtualSubstateId::CurrentEpoch,
        VirtualSubstate::CurrentEpoch(EARLIEST_ACTIVATION),
    );
    let transaction = council.call("retire", &[0, 1], vec![]);
    council.test.execute_expect_success(transaction, vec![]);

    let state = council.state();
    assert_eq!(state.rate_at(EARLIEST_ACTIVATION), Some(700));
    assert_eq!(state.rate_at(EARLIEST_ACTIVATION + 1), Some(700));
    assert_eq!(state.rate_at(EARLIEST_ACTIVATION + 2), None);
}

/// A foreign proposal from the previous epoch can be processed after a council transaction commits,
/// so that epoch's rate must resolve the same afterwards.
#[test]
fn setting_a_rate_keeps_the_previous_epoch_resolvable() {
    let mut council = Council::seated(3, 2);
    let transaction = council.set_burn_rate(&[0, 1], 700, EARLIEST_ACTIVATION);
    council.test.execute_expect_success(transaction, vec![]);

    council.test.set_virtual_substate(
        VirtualSubstateId::CurrentEpoch,
        VirtualSubstate::CurrentEpoch(EARLIEST_ACTIVATION),
    );
    let transaction = council.set_burn_rate(&[0, 1], 900, EARLIEST_ACTIVATION + 2);
    council.test.execute_expect_success(transaction, vec![]);

    council.test.set_virtual_substate(
        VirtualSubstateId::CurrentEpoch,
        VirtualSubstate::CurrentEpoch(EARLIEST_ACTIVATION + 2),
    );
    let transaction = council.set_burn_rate(&[0, 1], 50, EARLIEST_ACTIVATION + 4);
    council.test.execute_expect_success(transaction, vec![]);

    let state = council.state();
    assert_eq!(state.rate_at(EARLIEST_ACTIVATION + 1), Some(700));
    assert_eq!(state.rate_at(EARLIEST_ACTIVATION + 2), Some(900));
}

#[test]
fn a_council_can_rotate_itself() {
    let mut council = Council::seated(3, 2);
    let (new_member_secret, new_member_public) = council.test.new_key_pair(100);
    let new_member = new_member_public.to_byte_type();

    let transaction = council.call("set_council", &[0, 1], args![1u16, vec![new_member]]);
    council.test.execute_expect_success(transaction, vec![]);
    assert_eq!(council.owner_rule(), council_owner_rule(1, &[new_member]));

    // The outgoing members no longer satisfy the owner rule.
    let transaction = council.set_burn_rate(&[0, 1], 700, EARLIEST_ACTIVATION);
    let reason = council.test.execute_expect_failure(transaction, vec![]);
    assert_reject_reason(&reason, "Access Denied");

    council.members = vec![new_member_secret];
    let transaction = council.set_burn_rate(&[0], 700, EARLIEST_ACTIVATION);
    council.test.execute_expect_success(transaction, vec![]);
    assert_eq!(council.state().rate_at(EARLIEST_ACTIVATION), Some(700));
}

/// The engine rejects a threshold it would never admit, so a council cannot brick itself by setting
/// one — `retire` is the only way out.
#[test]
fn a_council_cannot_rotate_to_an_unsatisfiable_threshold() {
    let mut council = Council::seated(3, 2);
    let members = vec![council.public_key(0), council.public_key(1)];

    let transaction = council.call("set_council", &[0, 1], args![3u16, members]);
    let reason = council.test.execute_expect_failure(transaction, vec![]);
    assert_reject_reason(&reason, "threshold");

    // The sitting council is untouched.
    let transaction = council.set_burn_rate(&[0, 1], 700, EARLIEST_ACTIVATION);
    council.test.execute_expect_success(transaction, vec![]);
}
