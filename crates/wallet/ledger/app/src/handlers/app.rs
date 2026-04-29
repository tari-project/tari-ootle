//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::{constants::LEDGER_APP_NAME, state::State, status::AppStatus};

pub fn get_version(_state_mut: &mut State, _req: ()) -> Result<&'static [u8], AppStatus> {
    Ok(env!("CARGO_PKG_VERSION").as_bytes())
}

pub fn get_app_name(_state_mut: &mut State, _req: ()) -> Result<&'static [u8], AppStatus> {
    Ok(LEDGER_APP_NAME.as_bytes())
}
