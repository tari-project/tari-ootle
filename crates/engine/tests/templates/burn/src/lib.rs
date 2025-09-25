//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {

    use super::*;

    pub struct Burn {
        fungible: Vault,
        non_fungible: Vault,
        confidential: Vault,
        stealth: Vault,
    }

    impl Burn {
        pub fn new(confidential_supply: ConfidentialOutputStatement) -> Component<Self> {
            let fungible = ResourceBuilder::fungible()
                .burnable(rule!(allow_all))
                .initial_supply(1_000_000);

            let non_fungible = ResourceBuilder::non_fungible()
                .burnable(rule!(allow_all))
                .initial_supply((1..=10).map(NonFungibleId::from_u32));

            let confidential = ResourceBuilder::confidential()
                .burnable(rule!(allow_all))
                .initial_supply(confidential_supply);

            let stealth = ResourceBuilder::stealth()
                .burnable(rule!(allow_all))
                .initial_supply(1_000_000);

            Component::new(Self {
                fungible: Vault::from_bucket(fungible),
                non_fungible: Vault::from_bucket(non_fungible),
                confidential: Vault::from_bucket(confidential),
                stealth: Vault::from_bucket(stealth),
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn burn_all(&mut self) {
            let bucket = self.fungible.withdraw_all();
            bucket.burn();
            let bucket = self.stealth.withdraw_all();
            bucket.burn();
            let bucket = self.non_fungible.withdraw_all();
            bucket.burn();
            let bucket = self.confidential.withdraw_all();
            bucket.burn();
        }
    }
}
