//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![no_std]

extern crate alloc;

// Use talc as the global allocator
#[global_allocator]
static ALLOCATOR: talc::TalckWasm = unsafe { talc::TalckWasm::new_global() };

// WARN: dont use wee_alloc in production. https://github.com/rustwasm/wee_alloc/issues/106
// #[global_allocator]
// static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

// or lol_alloc
// use lol_alloc::{AssumeSingleThreaded, FreeListAllocator};

// SAFETY: Templates only run in a single thread.
// #[global_allocator]
// static ALLOCATOR: AssumeSingleThreaded<FreeListAllocator> =
//     unsafe { AssumeSingleThreaded::new(FreeListAllocator::new()) };

use tari_template_lib::prelude::*;

#[template]
mod template {
    use alloc::{format, string::String};

    use super::*;

    #[derive(Debug, Default)]
    pub struct NoStdCounter {
        value: u128,
    }

    impl NoStdCounter {
        pub fn simple() -> String {
            format!("Hello from no_std! This is demonstrates allocation and the engine deallocating memory correctly.")
        }

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

        pub fn panic_works() {
            panic!("Panic works in no_std!");
        }
    }
}
