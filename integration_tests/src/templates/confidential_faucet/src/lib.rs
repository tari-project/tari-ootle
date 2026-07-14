//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod confidential_faucet_template {
    use super::*;

    pub struct ConfidentialFaucet {
        vault: Vault,
    }

    impl ConfidentialFaucet {
        /// Creates a confidential resource whose initial supply is entirely revealed, so that a caller can take
        /// funds from it without having to construct a confidential proof.
        pub fn mint(initial_supply: Amount) -> Component<Self> {
            let coins = ResourceBuilder::confidential()
                .with_token_symbol("CONF")
                .mintable(rule!(allow_all), OWNER)
                .initial_supply(ConfidentialOutputStatement::mint_revealed(initial_supply));

            Component::new(Self {
                vault: Vault::from_bucket(coins),
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        /// Takes revealed funds out of the faucet. The recipient turns these into confidential outputs itself.
        pub fn take_free_coins(&mut self, amount: Amount) -> Bucket {
            debug!("Withdrawing {} revealed coins from the confidential faucet", amount);
            self.vault.withdraw(amount)
        }

        pub fn vault_balance(&self) -> Amount {
            self.vault.balance()
        }
    }
}
