//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_ledger_common::arg_types::{KeyType, SignMode, SigningField};

use crate::hashing::MessageHasher;

#[derive(Default)]
pub enum State {
    #[default]
    None,
    /// A `SignTransaction` stream is in progress: the message digest is being accumulated across
    /// APDU chunks and the display summary is being parsed from the same bytes.
    SigningTransaction(SigningState),
}

impl State {
    pub fn reset(&mut self) {
        *self = State::None;
    }
}

pub struct SigningState {
    /// Running transaction message digest, seeded with the domain preamble for `mode`.
    pub hasher: MessageHasher,
    pub account: u64,
    pub index: u64,
    pub key_type: KeyType,
    pub mode: SignMode,
    /// Index into the expected canonical field sequence for `mode`.
    pub field_cursor: usize,
    /// `Some` while the bytes of a large field are still arriving across multiple chunks.
    pub in_field: Option<SigningField>,
    /// Human-readable summary, parsed from the same bytes fed into `hasher`.
    pub display: TxDisplay,
}

/// Minimal transaction summary shown to the user before signing. Every value here is parsed from
/// the canonical preimage bytes that are hashed, so what is displayed is what is signed.
#[derive(Default, Clone, Copy)]
pub struct TxDisplay {
    pub network: u8,
    pub fee_instruction_count: u32,
    pub instruction_count: u32,
    pub input_count: u32,
    pub min_epoch: Option<u64>,
    pub max_epoch: Option<u64>,
}
