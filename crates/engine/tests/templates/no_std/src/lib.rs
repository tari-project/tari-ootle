//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![no_std]

extern crate alloc;

// use talc::*;

// TODO: could not get single threaded allocator to work (AssumeUnlockable)
// template errors with unreachable error.
// #[global_allocator]
// static ALLOCATOR: Talck<locking::AssumeUnlockable, ErrOnOom> = Talc::new(unsafe {
//     let a = Span::from_array(core::ptr::addr_of!(ARENA).cast_mut());
//     ErrOnOom
// })
// .lock();

// TODO: investigate this further
// Unfortunately, talc does not seem to work in non-trivial cases (probably something in our code not theirs)

// const MAX_MEM: usize = (632 - 17) * 64 * 1024; // 20 pages of 64KiB each
// Use Talc as our global heap allocator.
// static mut ARENA: [u8; MAX_MEM] = [0; MAX_MEM];
// #[global_allocator]
// static ALLOCATOR: Talck<spin::Mutex<()>, ClaimOnOom> = Talc::new(unsafe {
//     // if we're in a hosted environment, the Rust runtime may allocate before
//     // main() is called, so we need to initialize the arena automatically
//     ClaimOnOom::new(Span::from_array(core::ptr::addr_of!(ARENA).cast_mut()))
// })
// .lock();

// WARN: dont use weealloc in production. https://github.com/rustwasm/wee_alloc/issues/106
// We use it in the tests because it is the only no_std allocator we could get working.
// Use `wee_alloc` as the global allocator.
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

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
            format!("Hello from no_std! This is demonstrates allocation.")
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
            // debug!("Incrementing counter (current:{})", self.value);
            self.value += 1;
        }

        pub fn reset_to(&mut self, value: u128) {
            // debug!("Changing value from {:?} to {}", self.value, value);
            self.value = value;
        }

        pub fn panic_works() {
            panic!("Panic works in no_std!");
        }
    }
}
