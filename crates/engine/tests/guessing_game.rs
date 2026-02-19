//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::{
    prelude::ComponentAddress,
    types::{NonFungibleAddress, NonFungibleId},
};
use tari_template_test_tooling::{TemplateTest, support::assert_error::assert_reject_reason};

const TEMPLATE_PATHS: &[&str] = &["tests/templates/guessing_game"];
const TEMPLATE_NAME: &str = "GuessingGame";
const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn it_works() {
    let prize = NonFungibleId::from_string("💎");
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);

    let template = test.get_template_address(TEMPLATE_NAME);

    // The owner of the template starts a new game by calling the `start_game` method of the component. This will create
    // a new game instance and allocate a new component address for it. NOTE: for simplicity in tests, fees are
    // turned off by default. To turn them on use `test.enable_fees()`, you will then need to add fee instructions to
    // pay the fee.
    test.execute_expect_success(
        Transaction::builder_localnet()
            // Allocate a new component address
            .allocate_component_address("guessing_game")
            // Construct the component by calling the `new` function of the template, passing the address allocation as an argument
            .call_function(template, "new", args![Workspace("guessing_game")])
            // Start a new game
            .call_method("guessing_game", "start_game", args![prize])
            .build_and_seal(test.secret_key()),
        vec![test.owner_proof()],
    );

    // A game has started and the players can now make their guesses.
    // First, we need to find the component address of the game that was just started. In tests, we can do this by
    // querying the state store for components that were created from the template address.
    let (game_address, _) = test
        .read_only_state_store()
        .get_components_by_template_address(template)
        .unwrap()
        .remove(0);

    // Let's create some accounts for the players
    let (user1_account, _user1_proof, user1_secret) = test.create_empty_account();
    let (user2_account, _user2_proof, user2_secret) = test.create_empty_account();
    let (user3_account, _user3_proof, user3_secret) = test.create_empty_account();

    // Just to demonstrate, we'll test an "unhappy path": user 1 makes a bad guess, since our template requires guesses
    // between 0 and 10.
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_method(game_address, "guess", args![100, user1_account])
            .build_and_seal(&user1_secret),
        vec![],
    );

    // Matches the panic message in the template.
    assert_reject_reason(reason, "Guess must be from 0 to 10");

    // TIP: It is always a good idea to test all the various failure cases of a template in separate tests, but for
    // brevity we'll skip this here.

    // Let's make a correct guess with user 1
    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(game_address, "guess", args![5, user1_account])
            .build_and_seal(&user1_secret),
        vec![],
    );

    // User 2 makes a correct guess
    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(game_address, "guess", args![7, user2_account])
            .build_and_seal(&user2_secret),
        vec![],
    );

    // User 3 makes a correct guess
    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(game_address, "guess", args![3, user3_account])
            .build_and_seal(&user3_secret),
        vec![],
    );

    // Now let's end the game and check the results.
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(game_address, "end_game_and_payout", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    // Get the event out of the execution result.
    let event = result
        .finalize
        .events
        .iter()
        .find(|event| {
            // Template events are prefixed with the template name
            event.topic() == "GuessingGame.GameEnded"
        })
        .expect("GameEnded event not found");

    // It's difficult to test randomness, we have a 33% chance of a winner, so we'll have to assert either.
    match event.get_payload("winner_account") {
        Some(winner) => {
            // Someone won, let's assert that it's one of the three players.
            let winner = winner.parse::<ComponentAddress>().unwrap();
            let winner = [user1_account, user2_account, user3_account]
                .into_iter()
                .find(|w| *w == winner)
                .expect("Winner must be one of the three players");
            let account = test.read_only_state_store().get_account(winner).unwrap();
            let (_, vault) = account.vaults().first_key_value().unwrap();
            let vault = test.read_only_state_store().get_vault(&vault.vault_id()).unwrap();
            let nfts = vault.get_non_fungible_ids();
            assert!(nfts.contains(&prize), "Winner must have received the prize");
            eprintln!("Congratulations to the winner: {}", winner);
        },
        None => {
            // No winner, better luck next time!
            let resource_addr = test
                .read_only_state_store()
                .get_resources_by_owner(&test.to_public_key_bytes())
                .unwrap()
                .first()
                .map(|(addr, _)| *addr)
                .unwrap();
            let nft = test
                .read_only_state_store()
                .get_substate(&NonFungibleAddress::new(resource_addr, prize.clone()).into())
                .unwrap();
            assert!(
                nft.substate_value().as_non_fungible().unwrap().is_burnt(),
                "Prize must be burnt if there is no winner"
            );
            eprintln!("No winner this time, the prize was burnt: {}", prize);
        },
    }
}
