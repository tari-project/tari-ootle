//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
#![no_std]

use tari_template_lib::prelude::*;

#[template]
mod template {

    use super::*;

    pub struct Freeze {
        vault: Vault,
    }

    impl Freeze {
        pub fn new(address: ComponentAddressAllocation) -> (Component<Self>, ResourceAddress) {
            let bucket = ResourceBuilder::confidential()
                .freezable(rule!(allow_all))
                .initial_supply(ConfidentialOutputStatement::mint_revealed(1_000_000));

            let resource_address = bucket.resource_address();

            let component = Component::new(Self {
                vault: Vault::from_bucket(bucket),
            })
            .with_address_allocation(address)
            .with_access_rules(AccessRules::allow_all())
            .create();

            (component, resource_address)
        }

        pub fn withdraw(&mut self, amount: Amount) -> Bucket {
            self.vault.withdraw(amount)
        }

        pub fn freeze(&mut self, vault_id: VaultId) {
            ResourceManager::get(self.vault.resource_address()).freeze_vault(vault_id);
        }

        pub fn unfreeze(&mut self, vault_id: VaultId) {
            ResourceManager::get(self.vault.resource_address()).unfreeze_vault(vault_id);
        }
    }
}
