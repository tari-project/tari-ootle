//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

/// Keeper fee in basis points (100 bp = 1%). Applied to the stake amount before settlement.
const KEEPER_FEE_BPS: u64 = 50; // 0.5%

/// A single epoch-based coin-flip bet, created by `EpochBettingHouse::place_bet`.
///
/// # How it works
///
/// 1. **Place**: The `EpochBettingHouse` creates this component in epoch N, depositing the
///    player's stake into this vault.
/// 2. **Settle**: Anyone calls `settle` in epoch N+1 or later. The epoch hash — which was
///    unknowable at placement time — decides the outcome:
///    - **Win** (seed byte odd): the player's stake is returned, and the house pays an equal
///      prize via `EpochBettingHouse::pay_winner`, giving the player **2× their stake** total.
///    - **Loss** (seed byte even): the stake is forwarded to the house reserve via
///      `EpochBettingHouse::collect_loss`.
/// 3. **Keeper fee**: A third-party caller receives 0.5% of the stake as a fee, incentivising
///    automated settlement bots.
///
/// # Security
///
/// The epoch hash is derived from the L1 base-layer block hash at the epoch boundary. It is
/// fixed for the entire epoch and cannot be known or influenced before the epoch begins.
#[template]
mod epoch_bet_template {
    use super::*;

    pub struct EpochBet {
        /// The epoch in which the bet was placed.
        placement_epoch: u64,
        /// Vault holding the player's stake while the bet is open.
        stake_vault: Vault,
        /// Account component to receive winnings on a win.
        player_account: ComponentAddress,
        /// The `EpochBettingHouse` component that manages this bet.
        house: ComponentAddress,
        /// Whether the bet has already been settled.
        settled: bool,
    }

    impl EpochBet {
        /// Constructor called by `EpochBettingHouse::place_bet` via `TemplateManager`.
        ///
        /// # Arguments
        /// * `stake` – the player's wagered funds.
        /// * `player_account` – account to receive winnings.
        /// * `house` – the house component address, used for cross-component settlement calls.
        pub fn new(
            stake: Bucket,
            player_account: ComponentAddress,
            house: ComponentAddress,
        ) -> Component<Self> {
            let placement_epoch = Consensus::current_epoch();

            let access_rules = ComponentAccessRules::new()
                // Anyone can settle (keeper pattern).
                .method("settle", rule![allow_all])
                .method("is_settled", rule![allow_all])
                .method("placement_epoch", rule![allow_all]);

            Component::new(Self {
                placement_epoch,
                stake_vault: Vault::from_bucket(stake),
                player_account,
                house,
                settled: false,
            })
            .with_access_rules(access_rules)
            .create()
        }

        /// Settle the bet using the current epoch's hash as the randomness seed.
        ///
        /// Can be called by **anyone** once `placement_epoch + 1` has been reached.
        ///
        /// * **Win** (seed byte odd): the player's stake is returned to them and the house
        ///   pays an equal prize, giving the player `2× stake`.
        /// * **Loss** (seed byte even): the stake is forwarded to the house reserve.
        ///
        /// # Arguments
        /// * `caller_account` – account to receive the keeper fee. Ignored (no fee taken)
        ///   if the caller is the bettor themselves.
        pub fn settle(&mut self, caller_account: ComponentAddress) {
            assert!(!self.settled, "Bet has already been settled");

            let current_epoch = Consensus::current_epoch();
            assert!(
                current_epoch > self.placement_epoch,
                "Cannot settle a bet in the same epoch it was placed"
            );

            self.settled = true;

            let is_keeper = caller_account != self.player_account;

            // Deduct keeper fee (if applicable) before determining win/loss.
            if is_keeper {
                let fee = compute_keeper_fee(self.stake_vault.balance());
                let fee_bucket = self.stake_vault.withdraw(fee);
                ComponentManager::get(caller_account).invoke("deposit", args![fee_bucket]);
            }

            // Use the current epoch hash as the randomness seed.
            // This value was unknowable at placement time.
            let seed = Consensus::current_epoch_hash();
            let won = seed[0] % 2 == 1;

            let stake_amount = self.stake_vault.balance();

            if won {
                // Return the player's stake from this vault.
                let stake = self.stake_vault.withdraw_all();
                ComponentManager::get(self.player_account).invoke("deposit", args![stake]);
                // Ask the house to pay an equal prize directly to the player.
                ComponentManager::get(self.house)
                    .invoke("pay_winner", args![self.player_account, stake_amount]);

                emit_event("BetSettled", metadata![
                    "outcome" => "win",
                    "epoch" => current_epoch.to_string(),
                    "seed_byte" => seed[0].to_string(),
                    "is_keeper" => is_keeper.to_string(),
                ]);
            } else {
                // Forward the stake to the house reserve.
                let lost_stake = self.stake_vault.withdraw_all();
                ComponentManager::get(self.house).invoke("collect_loss", args![lost_stake]);

                emit_event("BetSettled", metadata![
                    "outcome" => "loss",
                    "epoch" => current_epoch.to_string(),
                    "seed_byte" => seed[0].to_string(),
                    "is_keeper" => is_keeper.to_string(),
                ]);
            }
        }

        /// Returns true if this bet has been settled.
        pub fn is_settled(&self) -> bool {
            self.settled
        }

        /// Returns the epoch in which the bet was placed.
        pub fn placement_epoch(&self) -> u64 {
            self.placement_epoch
        }
    }

    fn compute_keeper_fee(total: Amount) -> Amount {
        // fee = total * KEEPER_FEE_BPS / 10_000, minimum 1 microtari
        let fee = total * KEEPER_FEE_BPS / 10_000;
        fee.max(Amount::ONE)
    }
}
