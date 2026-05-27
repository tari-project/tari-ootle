//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![no_std]
#![no_main]

extern crate alloc;

mod constants;
pub(crate) mod handlers;
mod request;
mod status;

use ledger_device_sdk::io::Comm;

use crate::state::State;

mod device;

mod crypto;
mod hashing;
mod key_derive;
mod state;

#[cfg(not(any(
    target_os = "nanosplus",
    target_os = "nanox",
    target_os = "stax",
    target_os = "flex",
    target_os = "apex_p"
)))]
compile_error!("Unsupported target OS. This app supports Ledger Nano S Plus, Nano X, Stax, Flex, and Apex Pro.");

ledger_device_sdk::set_panic!(ledger_device_sdk::exiting_panic);

#[unsafe(no_mangle)]
extern "C" fn sample_main() {
    let mut comm = Comm::new().set_expected_cla(constants::CLA);

    device::init(&mut comm);

    device::show_menu_main(&mut comm);

    let mut state = State::default();

    loop {
        if let Some(req) = device::next_command(&mut comm) {
            device::handle_apdu_request(&mut state, req);
        }
    }
}
