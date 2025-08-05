//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {

    use super::*;

    pub struct StealthFaucet {
        stealth_manager: StealthResourceManager,
    }

    impl StealthFaucet {
        pub fn new(initial_supply: StealthMintStatement) -> Component<Self> {
            let resource_address = ResourceBuilder::stealth()
                .mintable(rule!(allow_all))
                .initial_supply(initial_supply);

            Component::new(Self {
                stealth_manager: resource_address.into(),
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn new_with_view_key(outputs: StealthMintStatement, view_key: RistrettoPublicKeyBytes) -> Component<Self> {
            let resource_address = ResourceBuilder::stealth()
                .mintable(rule!(allow_all))
                .with_view_key(view_key)
                .initial_supply(outputs);

            Component::new(Self {
                stealth_manager: resource_address.into(),
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn mint(&self, statement: StealthMintStatement) {
            self.stealth_manager.mint(statement);
        }
    }
}
