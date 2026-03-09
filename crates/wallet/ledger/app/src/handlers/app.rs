//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::status::AppStatus;

pub fn get_version(_req: ()) -> Result<&'static [u8], AppStatus> {
    Ok(env!("CARGO_PKG_VERSION").as_bytes())
}

pub fn get_app_name(_req: ()) -> Result<&'static [u8], AppStatus> {
    Ok(b"Ootle Ledger App")
}
