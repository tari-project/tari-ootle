//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![cfg_attr(not(feature = "std"), no_std)]

pub mod arg_types;

/// APDU instruction set for the Ootle Ledger app.
/// Byte values must match what `tari-ledger-client` sends (CLA = 0x80).
#[repr(u8)]
#[derive(Debug)]
pub enum Instruction {
    GetVersion = 0x01,
    GetAppName = 0x02,
    GetPublicKey = 0x03,
    SignTransaction = 0x04,
}

impl TryFrom<u8> for Instruction {
    type Error = ();

    fn try_from(ins: u8) -> Result<Self, Self::Error> {
        match ins {
            0x01 => Ok(Instruction::GetVersion),
            0x02 => Ok(Instruction::GetAppName),
            0x03 => Ok(Instruction::GetPublicKey),
            0x04 => Ok(Instruction::SignTransaction),

            _ => Err(()),
        }
    }
}

/// Ledger application status words.
#[repr(u16)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum OotleStatusWord {
    BadRequest = 0x01,
    EncodeResponseFail = 0x02,
    KeyDeriveFail = 0x03,
}

pub const OOTLE_STATUS_BASE: u16 = 0xB000;

impl OotleStatusWord {
    pub fn to_status(self) -> u16 {
        OOTLE_STATUS_BASE | self as u16
    }
}

impl TryFrom<u16> for OotleStatusWord {
    type Error = u16;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value & 0xFF00 != OOTLE_STATUS_BASE {
            return Err(value);
        }
        match value & 0x00FF {
            c if c == Self::BadRequest as u16 => Ok(Self::BadRequest),
            c if c == Self::EncodeResponseFail as u16 => Ok(Self::EncodeResponseFail),
            c if c == Self::KeyDeriveFail as u16 => Ok(Self::KeyDeriveFail),
            _ => Err(value),
        }
    }
}
