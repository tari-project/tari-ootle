//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod fungible_token {
    use super::*;

    pub struct FungibleToken {
        token: ResourceManager,
    }

    impl FungibleToken {
        pub fn create_fixed_supply(symbol: String, divisibility: u8, supply: Amount, metadata: Metadata) -> Bucket {
            ResourceBuilder::public_fungible()
                .with_owner_rule(OwnerRule::None)
                .with_metadata(metadata)
                .with_token_symbol(symbol)
                .with_divisibility(divisibility)
                .initial_supply(supply)
        }

        pub fn create_mintable(symbol: String, divisibility: u8, metadata: Metadata) -> Component<Self> {
            let address = CallerContext::allocate_component_address(None);
            let this_component_only = rule!(component(address.get_address()));

            let token = ResourceBuilder::public_fungible()
                .with_owner_rule(OwnerRule::None)
                .with_metadata(metadata)
                .with_token_symbol(symbol)
                .with_divisibility(divisibility)
                .mintable(this_component_only.clone(), LOCKED)
                .burnable(this_component_only, LOCKED)
                .build();

            Component::new(Self {
                token: ResourceManager::get(token),
            })
            .with_address_allocation(address)
            .with_access_rules(
                ComponentAccessRules::new()
                    .add_method_rule("resource_address", rule!(allow_all))
                    .add_method_rule("total_supply", rule!(allow_all))
                    .default(rule!(deny_all)),
            )
            .create()
        }

        pub fn mint(&self, amount: Amount) -> Bucket {
            self.token.mint_fungible(amount)
        }

        pub fn burn(&self, bucket: Bucket) {
            assert_eq!(
                bucket.resource_address(),
                self.token.resource_address(),
                "Not this token"
            );
            bucket.burn();
        }

        pub fn resource_address(&self) -> ResourceAddress {
            self.token.resource_address()
        }

        pub fn total_supply(&self) -> Amount {
            self.token.total_supply()
        }
    }
}
