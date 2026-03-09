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
    // SignTransaction = 0x04,
    // GetViewKey = 0x04,
    // GetDhSharedSecret = 0x05,
    // GetScriptSignature = 0x06,
    // GetScriptOffset = 0x07,
    // GetRawSchnorrSignature = 0x08,
}

impl TryFrom<u8> for Instruction {
    type Error = ();

    fn try_from(ins: u8) -> Result<Self, Self::Error> {
        match ins {
            0x01 => Ok(Instruction::GetVersion),
            0x02 => Ok(Instruction::GetAppName),
            0x03 => Ok(Instruction::GetPublicKey),
            // 0x04 => Ok(Instruction::SignTransaction),
            // 0x04 => Ok(Ins::GetViewKey),
            // 0x05 => Ok(Ins::GetDhSharedSecret),
            // 0x06 => Ok(Ins::GetScriptSignature),
            // 0x07 => Ok(Ins::GetScriptOffset),
            // 0x08 => Ok(Ins::GetRawSchnorrSignature),
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
    // WrongP1P2 = 0xB002,
    // InsNotSupported = 0xB003,
    // ScriptSignatureFail = 0xB004,
    // RawSchnorrSignatureFail = 0xB005,
    // SchnorrSignatureFail = 0xB006,
    // ScriptOffsetNotUnique = 0xB007,
    // KeyDeriveFromCanonical = 0xB009,
    // KeyDeriveFromUniform = 0xB00A,
    // RandomNonceFail = 0xB00B,
    // BadBranchKey = 0xB00C,
    // MetadataSignatureFail = 0xB00D,
    // WrongApduLength = 0x6e03, // See ledger-device-rust-sdk/ledger_device_sdk/src/io.rs:16
    // UserCancelled = 0x6e04,   // See ledger-device-rust-sdk/ledger_device_sdk/src/io.rs:16
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
        match value {
            0xB001 => Ok(OotleStatusWord::BadRequest),
            0xB002 => Ok(OotleStatusWord::EncodeResponseFail),
            0xB003 => Ok(OotleStatusWord::KeyDeriveFail),
            // 0xB002 => Ok(OotleStatusWord::WrongP1P2),
            // 0xB003 => Ok(OotleStatusWord::InsNotSupported),
            // 0xB004 => Ok(OotleStatusWord::ScriptSignatureFail),
            // 0xB005 => Ok(OotleStatusWord::RawSchnorrSignatureFail),
            // 0xB006 => Ok(OotleStatusWord::SchnorrSignatureFail),
            // 0xB007 => Ok(OotleStatusWord::ScriptOffsetNotUnique),
            // 0xB008 => Ok(OotleStatusWord::KeyDeriveFail),
            // 0xB009 => Ok(OotleStatusWord::KeyDeriveFromCanonical),
            // 0xB00A => Ok(OotleStatusWord::KeyDeriveFromUniform),
            // 0xB00B => Ok(OotleStatusWord::RandomNonceFail),
            // 0xB00C => Ok(OotleStatusWord::BadBranchKey),
            // 0xB00D => Ok(OotleStatusWord::MetadataSignatureFail),
            _ => Err(value),
        }
    }
}
