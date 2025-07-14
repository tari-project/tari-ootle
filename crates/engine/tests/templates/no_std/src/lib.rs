//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![no_std]

use tari_template_lib::{
    prelude::*,
    template_dependencies::rust::{format, vec::Vec},
};

#[template]
mod template {
    use super::*;

    #[derive(Default)]
    pub struct NoStdCounter {
        value: u128,
    }

    impl NoStdCounter {
        pub fn new() -> Component<Self> {
            Component::new(Self::default())
                .with_access_rules(AccessRules::new().default(rule!(allow_all)))
                .create()
        }

        pub fn with_address(address: ComponentAddressAllocation) -> Component<Self> {
            Component::new(Self::default())
                .with_access_rules(AccessRules::new().default(rule!(allow_all)))
                .with_address_allocation(address)
                .create()
        }

        pub fn increment(&mut self) {
            debug!("Incrementing counter (current:{})", self.value);
            self.value += 1;
        }

        pub fn reset_to(&mut self, value: u128) {
            debug!("Changing value from {:?} to {}", self.value, value);
            self.value = value;
        }
    }
}
