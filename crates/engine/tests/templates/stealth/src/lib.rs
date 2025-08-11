//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {

    use super::*;

    pub struct StealthFaucet {
        manager: ResourceManager,
        supply_vault: Vault,
    }

    impl StealthFaucet {
        pub fn new(
            component_addr: ComponentAddressAllocation,
            resource_addr: ResourceAddressAllocation,
            initial_supply: Amount,
            view_key: Option<RistrettoPublicKeyBytes>,
        ) -> Component<Self> {
            let bucket = ResourceBuilder::stealth()
                .with_address_allocation(resource_addr)
                .mintable(rule!(allow_all))
                .then(|builder| {
                    if let Some(key) = view_key {
                        builder.with_view_key(key)
                    } else {
                        builder
                    }
                })
                .initial_supply(initial_supply);

            Component::new(Self {
                manager: bucket.resource_address().into(),
                supply_vault: Vault::from_bucket(bucket),
            })
            .with_address_allocation(component_addr)
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn take_funds(&self, amount: Amount) -> Bucket {
            self.supply_vault.withdraw(amount)
        }

        pub fn mint(&self, amount: Amount) {
            let bucket = self.manager.mint_stealth(amount);
            self.supply_vault.deposit(bucket);
        }
    }
}
