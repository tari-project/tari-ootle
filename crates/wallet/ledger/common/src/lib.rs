//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![cfg_attr(not(feature = "std"), no_std)]

pub mod arg_types;

/// Constants defining the exact byte recipe a hardware signer reproduces to recompute a
/// transaction's signing message and Schnorr challenge. These mirror the domains used by
/// `tari_ootle_transaction`'s `transaction_hasher_v1` and `tari_crypto`'s `SchnorrSigChallenge`,
/// and are shared by the device implementation and the host reference test so both stay aligned.
pub mod signing {
    /// Domain for the Ootle transaction message digest (`transaction_hasher_v1`).
    pub const TX_DOMAIN: &str = "com.tari.ootle.transaction";
    pub const TX_DOMAIN_VERSION: u8 = 1;
    /// Label for an authorization ("add signer") signature message.
    pub const TX_LABEL_SIGNATURE: &str = "Signature";
    /// Label for a seal signature message.
    pub const TX_LABEL_SEAL: &str = "SealSignature";

    /// Domain for the `tari_crypto` Ristretto Schnorr challenge (`SchnorrSigChallenge`).
    pub const SCHNORR_DOMAIN: &str = "com.tari.schnorr_signature";
    pub const SCHNORR_DOMAIN_VERSION: u8 = 1;
    pub const SCHNORR_LABEL: &str = "challenge";

    /// Device-internal domain tag for deterministic (synthetic) nonce derivation. Not part of any
    /// network format — only the resulting signature must verify — so this is chosen freely.
    pub const NONCE_DOMAIN: &[u8] = b"com.tari.ootle.ledger.schnorr_nonce.v1";
}

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
    /// A streamed `SignTransaction` chunk arrived out of order or in an unexpected state.
    SignStreamError = 0x04,
    /// Hashing the transaction message or Schnorr challenge failed on-device.
    HashFail = 0x05,
    /// The user rejected the transaction on the device.
    UserRejected = 0x06,
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
            c if c == Self::SignStreamError as u16 => Ok(Self::SignStreamError),
            c if c == Self::HashFail as u16 => Ok(Self::HashFail),
            c if c == Self::UserRejected as u16 => Ok(Self::UserRejected),
            _ => Err(value),
        }
    }
}
