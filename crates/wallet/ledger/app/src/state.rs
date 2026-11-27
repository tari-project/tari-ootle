//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_ledger_common::arg_types::{KeyType, SignMode, SignStreamState};

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
    /// `Some` for a confidential (stealth) transfer: the spent UTXO's sender public nonce `R`. When
    /// set, signing uses the stealth-derived key `c + k` instead of the raw account key.
    pub stealth_public_nonce: Option<[u8; 32]>,
    pub stream: SignStreamState,
}
