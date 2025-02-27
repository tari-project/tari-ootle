//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {
    use super::*;

    pub struct ComponentAddressAllocationTest {}

        impl ComponentAddressAllocationTest {
            pub fn create(comp_addr: ComponentAddress) -> Component<Self> {
                Component::new(Self {}).with_address_allocation(comp_addr.into()).create()
            }
        }
}
