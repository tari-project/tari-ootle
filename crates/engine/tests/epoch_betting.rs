//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Integration tests for the epoch-betting template pair (`EpochBettingHouse` + `EpochBet`)
//! and the `Consensus::current_epoch_hash()` randomness beacon.
//!
//! Tests 4 and 5 from OIP-0002 live here (tests 1–3 and 6 are in `test.rs` mod consensus).

use tari_engine_types::virtual_substate::{VirtualSubstate, VirtualSubstateId};
use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::{Amount, constants::TARI_TOKEN};
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const TEMPLATE_PATHS: &[&str] = &[
    "tests/templates/epoch_betting_house",
    "tests/templates/epoch_betting",
];

/// Initial house reserve used in tests.
const HOUSE_RESERVE: u64 = 10_000_000;

/// Create a funded house component and return its address.
///
/// The house is seeded with `HOUSE_RESERVE` microtari from a freshly-created funded account,
/// using the `EpochBet` template address so the house can create bets.
fn setup_house(test: &mut TemplateTest) -> tari_template_lib::prelude::ComponentAddress {
    let house_template = test.get_template_address("EpochBettingHouse");
    let bet_template = test.get_template_address("EpochBet");

    // Create an operator account to fund the house.
    let (operator_account, operator_proof, operator_secret) = test.create_funded_account();

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(operator_account, "withdraw", args![TARI_TOKEN, HOUSE_RESERVE])
            .put_last_instruction_output_on_workspace("reserve")
            .call_function(house_template, "new", args![Workspace("reserve"), bet_template])
            .build_and_seal(&operator_secret),
        vec![operator_proof],
    );

    test.read_only_state_store()
        .get_components_by_template_address(house_template)
        .unwrap()
        .into_iter()
        .map(|(addr, _)| addr)
        .next()
        .expect("House component not found")
}

/// Place a bet through the house and return the resulting EpochBet component address.
fn place_bet(
    test: &mut TemplateTest,
    house: tari_template_lib::prelude::ComponentAddress,
    player_account: tari_template_lib::prelude::ComponentAddress,
    player_secret: &tari_crypto::ristretto::RistrettoSecretKey,
    player_proof: tari_template_lib::types::NonFungibleAddress,
    stake: u64,
) -> tari_template_lib::prelude::ComponentAddress {
    let bet_template = test.get_template_address("EpochBet");

    let addrs_before: std::collections::HashSet<_> = test
        .read_only_state_store()
        .get_components_by_template_address(bet_template)
        .unwrap()
        .into_iter()
        .map(|(addr, _)| addr)
        .collect();

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(player_account, "withdraw", args![TARI_TOKEN, stake])
            .put_last_instruction_output_on_workspace("stake")
            .call_method(house, "place_bet", args![Workspace("stake"), player_account])
            .build_and_seal(player_secret),
        vec![player_proof],
    );

    // Find the one new address that wasn't there before.
    let addrs_after: std::collections::HashSet<_> = test
        .read_only_state_store()
        .get_components_by_template_address(bet_template)
        .unwrap()
        .into_iter()
        .map(|(addr, _)| addr)
        .collect();
    let mut new_addrs: Vec<_> = addrs_after.difference(&addrs_before).copied().collect();
    assert_eq!(new_addrs.len(), 1, "Expected exactly one new EpochBet component");
    new_addrs.remove(0)
}

/// Test 4 from OIP-0002:
/// Calling `settle` in the same epoch as `place_bet` must be rejected with a clear error.
#[test]
fn test_settle_bet_requires_later_epoch() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    // Epoch 1: place the bet.
    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(1));

    let house = setup_house(&mut test);
    let (player_account, player_proof, player_secret) = test.create_funded_account();

    let bet_component = place_bet(&mut test, house, player_account, &player_secret, player_proof.clone(), 1_000);

    // Still epoch 1: settle must fail.
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_method(bet_component, "settle", args![player_account])
            .build_and_seal(&player_secret),
        vec![player_proof],
    );

    assert_reject_reason(reason, "Cannot settle a bet in the same epoch it was placed");
}

/// Test 5 from OIP-0002:
/// A third-party keeper can settle the bet and receives the keeper fee.
///
/// We force a win (seed byte odd) and verify:
/// - keeper balance increases by the fee,
/// - player balance increases by `2 × stake − fee`.
#[test]
fn test_keeper_can_settle() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    // Epoch 1: place bet.
    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(1));

    let house = setup_house(&mut test);
    let (player_account, player_proof, player_secret) = test.create_funded_account();
    let (keeper_account, keeper_proof, keeper_secret) = test.create_funded_account();

    let stake_amount: u64 = 100_000;
    let bet_component = place_bet(
        &mut test,
        house,
        player_account,
        &player_secret,
        player_proof.clone(),
        stake_amount,
    );

    // Record balances before settlement.
    let player_balance_before = test
        .read_only_state_store()
        .get_vaults_for_account(player_account)
        .unwrap()
        .remove(&TARI_TOKEN)
        .unwrap()
        .balance();

    let keeper_balance_before = test
        .read_only_state_store()
        .get_vaults_for_account(keeper_account)
        .unwrap()
        .remove(&TARI_TOKEN)
        .unwrap()
        .balance();

    // Epoch 2: first byte odd → player wins.
    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(2));
    let mut win_hash = [0u8; 32];
    win_hash[0] = 0x01; // odd → win
    test.set_virtual_substate(
        VirtualSubstateId::CurrentEpochHash,
        VirtualSubstate::CurrentEpochHash(win_hash),
    );

    // Keeper settles the bet.
    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(bet_component, "settle", args![keeper_account])
            .build_and_seal(&keeper_secret),
        vec![keeper_proof],
    );

    // fee = stake * 50 / 10_000, min 1
    let expected_fee = Amount::from((stake_amount * 50 / 10_000).max(1));

    let keeper_balance_after = test
        .read_only_state_store()
        .get_vaults_for_account(keeper_account)
        .unwrap()
        .remove(&TARI_TOKEN)
        .unwrap()
        .balance();

    assert_eq!(
        keeper_balance_after,
        keeper_balance_before + expected_fee,
        "Keeper should receive the keeper fee"
    );

    // Player receives: stake (from vault) + prize (from house) − fee
    // = (stake − fee) + (stake − fee) … wait: fee comes off the stake vault first, then the
    // remaining stake is returned + the house matches that remaining amount.
    // So player gets: 2 × (stake − fee).
    let stake_after_fee = Amount::from(stake_amount) - expected_fee;
    let expected_player_gain = stake_after_fee + stake_after_fee; // 2× the net stake

    let player_balance_after = test
        .read_only_state_store()
        .get_vaults_for_account(player_account)
        .unwrap()
        .remove(&TARI_TOKEN)
        .unwrap()
        .balance();

    assert_eq!(
        player_balance_after,
        player_balance_before + expected_player_gain,
        "Player should receive 2× the net stake (after keeper fee) on a win"
    );
}

/// Additional test: the bettor settles their own winning bet — no keeper fee taken.
#[test]
fn test_player_settles_own_bet_win() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(1));

    let house = setup_house(&mut test);
    let (player_account, player_proof, player_secret) = test.create_funded_account();
    let stake_amount: u64 = 50_000;

    let player_balance_before = test
        .read_only_state_store()
        .get_vaults_for_account(player_account)
        .unwrap()
        .remove(&TARI_TOKEN)
        .unwrap()
        .balance();

    let bet_component = place_bet(
        &mut test,
        house,
        player_account,
        &player_secret,
        player_proof.clone(),
        stake_amount,
    );

    // Epoch 2, winning hash.
    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(2));
    let mut win_hash = [0u8; 32];
    win_hash[0] = 0x03; // odd → win
    test.set_virtual_substate(
        VirtualSubstateId::CurrentEpochHash,
        VirtualSubstate::CurrentEpochHash(win_hash),
    );

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(bet_component, "settle", args![player_account])
            .build_and_seal(&player_secret),
        vec![player_proof],
    );

    let player_balance_after = test
        .read_only_state_store()
        .get_vaults_for_account(player_account)
        .unwrap()
        .remove(&TARI_TOKEN)
        .unwrap()
        .balance();

    // No keeper fee; player receives 2× stake.
    assert_eq!(
        player_balance_after,
        player_balance_before + Amount::from(stake_amount),
        "Player should net +stake (received 2× stake back; only lost stake on placement)"
    );
}

/// Additional test: player settles their own losing bet — stake goes to house, player loses.
#[test]
fn test_player_settles_own_bet_loss() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(1));

    let house = setup_house(&mut test);
    let (player_account, player_proof, player_secret) = test.create_funded_account();
    let stake_amount: u64 = 50_000;

    let player_balance_before = test
        .read_only_state_store()
        .get_vaults_for_account(player_account)
        .unwrap()
        .remove(&TARI_TOKEN)
        .unwrap()
        .balance();

    let house_reserve_before: Amount = test.call_method(house, "reserve_balance", args![], vec![]);

    let bet_component = place_bet(
        &mut test,
        house,
        player_account,
        &player_secret,
        player_proof.clone(),
        stake_amount,
    );

    // Epoch 2, losing hash.
    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(2));
    let mut loss_hash = [0u8; 32];
    loss_hash[0] = 0x02; // even → loss
    test.set_virtual_substate(
        VirtualSubstateId::CurrentEpochHash,
        VirtualSubstate::CurrentEpochHash(loss_hash),
    );

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(bet_component, "settle", args![player_account])
            .build_and_seal(&player_secret),
        vec![player_proof],
    );

    let player_balance_after = test
        .read_only_state_store()
        .get_vaults_for_account(player_account)
        .unwrap()
        .remove(&TARI_TOKEN)
        .unwrap()
        .balance();

    // Player lost the stake.
    assert_eq!(
        player_balance_after,
        player_balance_before - Amount::from(stake_amount),
        "Player should lose the stake on a losing bet"
    );

    // House reserve grows by exactly the stake amount.
    let house_reserve_after: Amount = test.call_method(house, "reserve_balance", args![], vec![]);
    assert_eq!(
        house_reserve_after,
        house_reserve_before + Amount::from(stake_amount),
        "House reserve should increase by the lost stake"
    );
}

/// Additional test: a settled bet cannot be settled again.
#[test]
fn test_cannot_settle_twice() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(1));

    let house = setup_house(&mut test);
    let (player_account, player_proof, player_secret) = test.create_funded_account();

    let bet_component = place_bet(&mut test, house, player_account, &player_secret, player_proof.clone(), 1_000);

    // Advance epoch.
    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(2));

    // First settle — succeeds.
    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(bet_component, "settle", args![player_account])
            .build_and_seal(&player_secret),
        vec![player_proof.clone()],
    );

    // Second settle — must fail.
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_method(bet_component, "settle", args![player_account])
            .build_and_seal(&player_secret),
        vec![player_proof],
    );

    assert_reject_reason(reason, "Bet has already been settled");
}
