//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

/// The house component for epoch-based coin-flip betting.
///
/// # Roles
///
/// * **Operator** – deploys the house, funds the reserve, and sets the `EpochBet` template address.
/// * **Player** – calls `place_bet` to create a new [`EpochBet`] component and lock their stake.
/// * **EpochBet** – calls `pay_winner` on a win and `collect_loss` on a loss.
///
/// # Economics
///
/// Players bet an amount `X` against the house.
/// - **Win** (epoch-hash seed byte is odd): the player receives `2X` total — their original
///   stake (returned by the `EpochBet` component) plus a matching prize `X` paid by the house.
/// - **Loss** (epoch-hash seed byte is even): the player forfeits their stake `X` to the house.
///
/// The house reserve must hold at least `X` for each open bet (to cover potential wins).
/// Losses are automatically recycled back into the reserve, sustaining future payouts.
#[template]
mod epoch_betting_house_template {
    use super::*;

    pub struct EpochBettingHouse {
        /// Liquidity pool used to pay winning bets.
        reserve: Vault,
        /// Resource type accepted as bets.
        resource: ResourceAddress,
        /// Template address of the companion `EpochBet` template used to create per-bet components.
        bet_template: TemplateAddress,
    }

    impl EpochBettingHouse {
        /// Deploy a new house.
        ///
        /// # Arguments
        /// * `initial_reserve` – seed funds; determines the maximum sum of all open bets.
        /// * `bet_template` – template address of the `EpochBet` template that will be
        ///   instantiated for each new bet.
        pub fn new(initial_reserve: Bucket, bet_template: TemplateAddress) -> Component<Self> {
            let resource = initial_reserve.resource_address();

            let access_rules = ComponentAccessRules::new()
                .method("place_bet", rule![allow_all])
                .method("pay_winner", rule![allow_all])
                .method("collect_loss", rule![allow_all])
                .method("reserve_balance", rule![allow_all])
                .method("deposit_liquidity", rule![allow_all]);

            Component::new(Self {
                reserve: Vault::from_bucket(initial_reserve),
                resource,
                bet_template,
            })
            .with_access_rules(access_rules)
            .create()
        }

        /// Add more liquidity to the house reserve (owner only).
        pub fn deposit_liquidity(&mut self, funds: Bucket) {
            assert_eq!(
                funds.resource_address(),
                self.resource,
                "Wrong resource type for house reserve"
            );
            self.reserve.deposit(funds);
        }

        /// Returns the current reserve balance.
        pub fn reserve_balance(&self) -> Amount {
            self.reserve.balance()
        }

        /// Place a new bet against this house.
        ///
        /// Validates that the house has sufficient reserve to cover a potential win, then
        /// creates and returns a new `EpochBet` component holding the player's stake.
        ///
        /// # Arguments
        /// * `stake` – the funds the player is wagering.
        /// * `player_account` – account to receive winnings on a win.
        pub fn place_bet(
            &mut self,
            stake: Bucket,
            player_account: ComponentAddress,
        ) -> ComponentAddress {
            assert_eq!(
                stake.resource_address(),
                self.resource,
                "Bet must use the house's resource type"
            );
            assert!(!stake.amount().is_zero(), "Stake must be non-zero");
            // House must be able to cover the win payout (= prize equal to stake).
            assert!(
                self.reserve.balance() >= stake.amount(),
                "House reserve is insufficient to cover this bet"
            );

            let house = CallerContext::current_component_address();
            // Create a new EpochBet component via the stored template address.
            TemplateManager::get(self.bet_template).call("new", args![stake, player_account, house])
        }

        /// Called by an `EpochBet` component when the player wins.
        ///
        /// Withdraws `prize_amount` from the reserve and deposits it directly into
        /// `player_account`, providing the house-funded half of the `2X` payout.
        pub fn pay_winner(&mut self, player_account: ComponentAddress, prize_amount: Amount) {
            assert!(
                self.reserve.balance() >= prize_amount,
                "House reserve too low to pay prize"
            );
            let prize = self.reserve.withdraw(prize_amount);
            ComponentManager::get(player_account).invoke("deposit", args![prize]);
        }

        /// Called by an `EpochBet` component when the player loses.
        ///
        /// Deposits the forfeited stake into the house reserve.
        pub fn collect_loss(&mut self, stake: Bucket) {
            assert_eq!(
                stake.resource_address(),
                self.resource,
                "Wrong resource type for house reserve"
            );
            self.reserve.deposit(stake);
        }
    }
}
