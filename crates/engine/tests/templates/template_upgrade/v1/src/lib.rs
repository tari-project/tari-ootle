//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::{prelude::*, types::MaxVec};

#[template]
mod template {

    use super::*;

    pub struct TemplateV1 {
        signers: MaxVec<10, RistrettoPublicKeyBytes>,
        manager: ResourceManager,
        supply_vault: Vault,
    }

    impl TemplateV1 {
        pub fn new(owner_rule: OwnerRule, signers: MaxVec<10, RistrettoPublicKeyBytes>) -> Component<Self> {
            let resource_address = ResourceBuilder::non_fungible().mintable(rule!(allow_all), OWNER).build();

            Component::new(Self {
                signers,
                manager: resource_address.into(),
                supply_vault: Vault::new_empty(resource_address),
            })
            .with_owner_rule(owner_rule)
            .with_access_rules(AccessRules::allow_all())
            .create()
        }
    }
}
