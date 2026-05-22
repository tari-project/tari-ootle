//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![no_std]

extern crate alloc;

// Use talc as the global allocator
#[cfg(target_arch = "wasm32")]
#[global_allocator]
static TALC: talc::wasm::WasmDynamicTalc = talc::wasm::new_wasm_dynamic_allocator();

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
        // u64 rather than u128: minicbor 2.2 does not impl Encode/Decode for u128/i128 and the
        // workspace's bignum bridge (`Value::Integer(i128)` via `serde_bridge`) doesn't apply to
        // bare template struct fields. The test exists to exercise no_std + allocator, not 128-bit.
        value: u64,
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

        pub fn reset_to(&mut self, value: u64) {
            debug!("Changing value from {:?} to {}", self.value, value);
            self.value = value;
        }

        pub fn panic_works() {
            panic!("Panic works in no_std!");
        }
    }
}
