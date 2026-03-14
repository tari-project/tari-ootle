//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ledger_device_sdk::hash::{HashError, HashInit, blake2::Blake2b_512};

pub enum State {
    Normal,
    TransactionSigning(TransactionSigningState),
}

pub struct TransactionSigningState {
    hash_state: Blake2b_512,
}

impl TransactionSigningState {
    pub fn new() -> Self {
        Self {
            hash_state: Blake2b_512::new(),
        }
    }

    pub fn update(&mut self, data: &[u8]) -> Result<(), HashError> {
        self.hash_state.update(data)
    }

    pub fn finalize(mut self) -> Result<[u8; 64], HashError> {
        let mut buf = [0u8; 64];
        self.hash_state.finalize(&mut buf)?;
        Ok(buf)
    }
}
