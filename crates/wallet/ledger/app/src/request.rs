//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ledger_device_sdk::io::{ApduHeader, StatusWords};
pub use ootle_ledger_common::Instruction;

pub struct Request {
    #[allow(dead_code)]
    pub header: ApduHeader,
    pub instruction: Instruction,
}

impl TryFrom<ApduHeader> for Request {
    type Error = StatusWords;

    fn try_from(header: ApduHeader) -> Result<Self, Self::Error> {
        Ok(Request {
            instruction: header.ins.try_into().map_err(|_| StatusWords::BadIns)?,
            header,
        })
    }
}
