//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {
    use std::{collections::HashMap, mem};

    use super::*;

    const MAXIMUM_GUESSES_PER_ROUND: usize = 5;

    /// The guessing game component.
    ///
    /// This component contains a vault for the prize NFT, the guesses and a number for the current round.
    pub struct GuessingGame {
        /// A vault containing the prize to be won for the round, or is empty if no round is active
        prize_vault: Vault,
        /// Contains up to 5 guesses from different users
        guesses: HashMap<RistrettoPublicKeyBytes, Guess>,
        round_number: u32,
    }

    impl GuessingGame {
        /// Constructs a new Guessing Game component.
        ///
        /// The caller must provide a component address allocation.
        /// This allows the component to be created and called within one transaction instead of one transaction to
        /// create the component and others to call it with the new address. While this is not strictly necessary, it is
        /// good practice to provide this in constructors.
        pub fn new(address: ComponentAddressAllocation) -> Component<Self> {
            // Create a new NFT Resource that will be awarded to winners
            let prize_resource = ResourceBuilder::non_fungible()
                // Optionally give it a name
                .metadata("name", "Guessing Game Prize")
                // Optionally, provide a token symbol that exchanges and explorers will display for the new resource
                .with_token_symbol("🎲")
                // Create the resource with no supply (use `.initial_supply(...)` to mint new NFTs in the builder)
                .build();

            // By default, all component methods are restricted and can only be
            // called by the owner that created it.
            // For our game, that is perfect for starting and ending a round,  we want to allow anybody to place a
            // guess.
            let access_rules = ComponentAccessRules::new()
                // Here we allow anyone to call the "guess" method.
                .method("guess", rule![allow_all]);

            // Construct the component
            Component::new(Self {
                // We create an empty vault that will hold our prize NFT
                prize_vault: Vault::new_empty(prize_resource),
                guesses: HashMap::new(),
                round_number: 0,
            })
            .with_address_allocation(address)
            .with_access_rules(access_rules)
            .create()
        }

        /// Starts a new game and mint the prize to be won.
        ///
        /// Callable by: the component owner
        ///
        /// # Panics
        ///
        /// Panics if a round is already started.
        pub fn start_game(&mut self, prize: NonFungibleId) {
            assert!(!self.is_game_in_progress(), "Game already in progress!");
            self.round_number += 1;
            // To mint the prize, we need a ResourceManager. For convenience, a Vault provides the
            // `get_resource_manager` method that returns a resource manager for the resource it holds.
            let manager = self.prize_vault.get_resource_manager();
            // Mine the prize. Each NFT has immutable (cannot be changed) data and mutable (holder can change) data.
            // We pass in the round number as the immutable data to mint this round in NFT history
            // and empty (unit) for the mutable data.
            let prize = manager.mint_non_fungible(prize, &metadata!["round" => self.round_number.to_string()], &());
            self.prize_vault.deposit(prize);
        }

        /// Places a guess. If the guess is correct and selected to win, payouts will be made into the provided
        /// component. NOTE: this component must have a `deposit(bucket: Bucket)` function (typically a built-in
        /// Account component)
        ///
        /// Callable by: anyone
        ///
        /// # Panics
        ///
        /// Panics if the guess is not between 0 and 10, if the player has already made a guess this round, if the
        /// maximum number of guesses has already been reached or if no game is in progress.
        pub fn guess(&mut self, guess: u8, payout_to: ComponentAddress) {
            assert!(guess <= 10, "Guess must be from 0 to 10");
            assert!(
                self.guesses.len() < MAXIMUM_GUESSES_PER_ROUND,
                "No more guesses allowed"
            );
            assert!(self.is_game_in_progress(), "No game has been started");

            // We'll get the signer of the transaction to use as the player identifier for the guess.
            let player = CallerContext::transaction_signer_public_key();
            // Create a ComponentManager. This is a wrapper around a component address that allows us
            // to call methods on the component. We'll use this in end_game_and_payout.
            let payout_to = ComponentManager::get(payout_to);
            // Insert the guess into the hashmap, assert that the player hasn't already made a guess this round.
            let maybe_previous_guess = self.guesses.insert(player, Guess { payout_to, guess });
            assert!(maybe_previous_guess.is_none(), "You already guessed in this round");
        }

        /// Ends the game, determines the winner and pays out the prize. If there are multiple winners, the first one
        /// found will win. If there are no winners, the prize is burned.
        ///
        /// Callable by: the component owner
        ///
        /// # Panics
        ///
        /// Panics if no game is in progress.
        pub fn end_game_and_payout(&mut self) {
            // Withdraw the prize into `Bucket`. A Bucket is a container for a single resource and is used to pass
            // resources around. A bucket MUST either be deposited into a vault or burnt by the end of the transaction.
            // You can also return buckets which allows them to be used at the transaction-level.
            // In fact, returning them is typically how you'd use buckets but that doesn't work in this case, since we
            // don't know the winner ahead of time.
            //
            // This will panic if there is no prize in the vault, which also means that no game is in progress.
            let mut prize = self.prize_vault.withdraw(1u64);

            // Generate a (pseudo) random number to determine the winner.
            let number = generate_number();

            // Take the guesses and reset the guesses for the next round.
            let guesses = mem::take(&mut self.guesses);
            let num_participants = guesses.len();

            for (player, guess) in guesses {
                if guess.guess == number {
                    // We have a winner! Payout the prize to the component specified in the guess.
                    // This is a cross component call that invokes the `deposit` method on the component specified in
                    // the guess. We pass in the bucket containing the prize as an argument.
                    guess.payout_to.invoke("deposit", args![prize]);
                    // Emit an event with the winner and some game metadata.
                    // Events are a great way to provide information about what happened during a transaction execution.
                    // They can be indexed and queried by explorers and other tools.
                    emit_event("GameEnded", metadata![
                        "winner" => player.to_string(),
                        "winner_account" => guess.payout_to.component_address().to_string(),
                        "number" => number.to_string(),
                        "num_participants" => num_participants.to_string(),
                    ]);
                    return;
                }
            }

            // No winner, bye bye prize!
            emit_event(
                "GameEnded",
                metadata!["number" => number.to_string(), "num_participants"  => num_participants.to_string()],
            );
            prize.burn();
        }

        fn is_game_in_progress(&self) -> bool {
            // If the prize vault has an NFT, then game on!
            !self.prize_vault.balance().is_zero()
        }
    }

    pub struct Guess {
        pub payout_to: ComponentManager,
        pub guess: u8,
    }

    fn generate_number() -> u8 {
        use tari_template_lib::rand::random_bytes;
        let num = random_bytes(1)[0];
        // Squish it to between 0 and 10
        num % 11
    }
}
