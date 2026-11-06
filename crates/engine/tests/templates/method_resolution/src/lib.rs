//   Copyright 2026. The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A component whose methods only its owner may call, holding a resource that only this template's code may mint.

use tari_template_lib::prelude::*;

#[template]
mod method_resolution {
    use super::*;

    pub struct MethodResolution {
        tokens: Vault,
        value: u32,
    }

    impl MethodResolution {
        pub fn create(template_address: TemplateAddress) -> Component<MethodResolution> {
            let tokens = ResourceBuilder::public_fungible()
                .mintable(rule!(template(template_address)), LOCKED)
                .initial_supply(1000u32);

            Component::new(Self {
                tokens: Vault::from_bucket(tokens),
                value: 7,
            })
            .with_owner_rule(OwnerRule::OwnedBySigner)
            .with_access_rules(
                ComponentAccessRules::new()
                    .default(rule!(deny_all))
                    .add_method_rule("version", rule!(allow_all)),
            )
            .create()
        }

        pub fn version() -> u32 {
            1
        }

        pub fn get_value(&self) -> u32 {
            self.value
        }

        pub fn mint_more(&self, amount: Amount) -> Bucket {
            ResourceManager::get(self.tokens.resource_address()).mint_fungible(amount)
        }
    }
}
