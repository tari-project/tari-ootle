//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {
    use super::*;

    pub struct ResourceAddressAllocationTest {
        address: ResourceAddress,
        vault: Vault,
    }

    impl ResourceAddressAllocationTest {
        pub fn create() -> (Component<Self>, ResourceAddress) {
            // Create the non-fungible resource with 1 token (optional)
            let tokens = [
                NonFungibleId::from_u32(1),
                NonFungibleId::from_u64(u64::MAX),
                NonFungibleId::from_string("Sparkle1"),
                NonFungibleId::from_u256([0u8; 32]),
            ];

            // allocate a new resource address
            let allocation = CallerContext::allocate_address(args::SubstateType::Resource, None)
                .as_resource_address_allocation().expect("we must have a resource address allocation");
            let address = allocation.address().clone();

            let bucket = ResourceBuilder::non_fungible()
                .with_token_symbol("RAAT")
                .with_address_allocation(allocation)
                // Allow minting and burning for tests
                .mintable(rule!(allow_all))
                .burnable(rule!(allow_all))
                .initial_supply(tokens);

            (
                Component::new(Self {
                    address: bucket.resource_address(),
                    vault: Vault::from_bucket(bucket),
                })
                    .with_access_rules(AccessRules::allow_all())
                    .create(),
                address
            )
        }

        pub fn drop_allocation() {
            let _allocation = CallerContext::allocate_address(args::SubstateType::Resource, None)
                .as_resource_address_allocation().expect("we must have a resource address allocation");
        }
    }
}
