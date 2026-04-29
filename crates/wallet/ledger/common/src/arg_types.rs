//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::{BorshDeserialize, BorshSerialize};

#[repr(u64)]
#[derive(Clone, Copy, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
pub enum KeyType {
    /// The account key branch, used for deriving account keys.
    Account = 0x00,
    /// The transaction key branch, used to sign transactions that do not need to be signed with the account key.
    Transaction = 0x01,
    /// The Elgamal encryption view key branch, used to derive a view key for resources with "viewable balance"
    /// enabled.
    ElgamalEncryptionViewKey = 0x02,
    /// The stealth mask branch, used to derive masks for stealth addresses.
    StealthMask = 0x03,
    /// The confidential mask branch, used to derive masks for confidential transactions.
    ConfidentialMask = 0x04,
    /// Used to generate nonces that need to be recreated later, e.g. to derive the DH secret for claim burn
    Nonce = 0x05,
    /// Branch used to derive view-only keys. This key is used to derive an encryption key for wallet recovery. But
    /// does not allow spending.
    ViewOnlyKey = 0x06,
}

impl KeyType {
    pub fn as_u64(&self) -> u64 {
        *self as u64
    }
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct GetPublicKeyRequest {
    pub account: u64,
    pub index: u64,
    pub key_type: KeyType,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct GetPublicKeyResponse {
    pub public_key: [u8; 32],
}

/// P1 parameter values for APDU commands with large payloads.
pub enum PayloadChunk {
    /// First chunk (header)
    First = 0x00,
    /// Complete instruction (fits in one APDU)
    Instruction = 0x01,
    /// Start of large instruction payload
    PayloadStart = 0x02,
    /// Continuation of large payload
    PayloadContinuation = 0x03,
    /// OR'd with any above to signal final chunk
    Last = 0x80,
}

// pub struct SignTransactionHeader {
//     tx_schema_version: u16,
//     num_inputs: u16,
// }
