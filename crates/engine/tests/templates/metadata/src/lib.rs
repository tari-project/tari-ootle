//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {
    use super::*;

    pub struct MetadataTest {
        fungible: Vault,
    }

    impl MetadataTest {
        pub fn new_with_symbol() -> Component<Self> {
            let fungible = ResourceBuilder::public_fungible()
                .with_token_symbol("FOO")
                .update_metadata(rule!(allow_all))
                .initial_supply(1000u32);
            Component::new(Self {
                fungible: Vault::from_bucket(fungible),
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn new_without_symbol() -> Component<Self> {
            let fungible = ResourceBuilder::public_fungible()
                .update_metadata(rule!(allow_all))
                .initial_supply(1000u32);
            Component::new(Self {
                fungible: Vault::from_bucket(fungible),
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn resource_address(&self) -> ResourceAddress {
            self.fungible.resource_address()
        }

        pub fn set_metadata(&self, metadata: Metadata) {
            ResourceManager::get(self.fungible.resource_address()).set_metadata(metadata);
        }
    }
}
