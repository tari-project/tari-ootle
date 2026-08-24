//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {
    use tari_template_lib::models::AddressAllocationId;

    use super::*;

    pub struct AddressAllocationTest {
        // comp_addr: ComponentAddress,
        resource: ResourceAddress,
        vault: Vault,
    }

    impl AddressAllocationTest {
        pub fn create() -> Component<Self> {
            let component_allocation = CallerContext::allocate_component_address(None);
            let resource_allocation = CallerContext::allocate_resource_address();
            Self::create_from_allocations(component_allocation, resource_allocation)
        }

        pub fn create_from_allocations(
            comp_alloc: ComponentAddressAllocation,
            resource: ResourceAddressAllocation,
        ) -> Component<Self> {
            // Create the non-fungible resource with 1 token (optional)
            let tokens = [
                NonFungibleId::from_u32(1),
                NonFungibleId::from_u64(u64::MAX),
                NonFungibleId::from_string("AAFIT"),
                NonFungibleId::from_u256([0u8; 32]),
            ];

            let bucket = ResourceBuilder::non_fungible()
                .with_token_symbol("AAFIT")
                .with_address_allocation(resource)
                // Allow minting and burning for tests
                .mintable(rule!(allow_all), OWNER)
                .burnable(rule!(allow_all), OWNER)
                .initial_supply(tokens);

            Component::new(Self {
                resource: bucket.resource_address(),
                vault: Vault::from_bucket(bucket),
                // You cannot store your own component address - ATM I think that is a good thing
                // comp_addr: *comp_alloc.address(),
            })
            .with_address_allocation(comp_alloc)
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn get_resource_address(&self) -> ResourceAddress {
            self.resource
        }

        pub fn get_component_allocation_address(comp_alloc: ComponentAddressAllocation) -> String {
            // You can't return the actual address until the component is created
            comp_alloc.get_address().to_string()
        }

        pub fn get_resource_allocation_address(alloc: ResourceAddressAllocation) -> String {
            // You can't return the actual address until the resource is created
            alloc.get_address().to_string()
        }

        pub fn drop_resource_allocation() {
            let _allocation = CallerContext::allocate_resource_address();
        }

        pub fn drop_component_allocation() {
            let _allocation = CallerContext::allocate_component_address(None);
        }

        pub fn cross_template_call_with_ids(
            template_addr: TemplateAddress,
            comp_alloc: AddressAllocationId,
            resource_alloc: AddressAllocationId,
        ) -> Component<Self> {
            let comp_alloc = ComponentAddressAllocation::new(comp_alloc);
            let resource_alloc = ResourceAddressAllocation::new(resource_alloc);
            // Will fail, because the allocs are not in scope
            Self::cross_template_call_with_allocs(template_addr, comp_alloc, resource_alloc)
        }

        pub fn cross_template_call_with_allocs(
            template_addr: TemplateAddress,
            comp_alloc: ComponentAddressAllocation,
            resource_alloc: ResourceAddressAllocation,
        ) -> Component<Self> {
            TemplateManager::get(template_addr).call("create_from_allocations", args![comp_alloc, resource_alloc])
        }

        pub fn attempt_access_to_component_alloc_addr(comp_alloc: AddressAllocationId) {
            let comp_alloc = ComponentAddressAllocation::new(comp_alloc);
            comp_alloc.get_address();
        }

        pub fn attempt_access_to_resource_alloc_addr(resource: AddressAllocationId) {
            let resource_alloc = ResourceAddressAllocation::new(resource);
            resource_alloc.get_address();
        }
    }
}
