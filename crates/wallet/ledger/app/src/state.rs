//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

// use ledger_device_sdk::hash::{HashError, HashInit, blake2::Blake2b_512};

#[derive(Default)]
pub enum State {
    #[default]
    None,
    TransactionUpload(TransactionUploadState),
}

impl State {
    pub fn reset(&mut self) {
        *self = State::None;
    }
}

pub struct TransactionUploadState {}

impl TransactionUploadState {
    pub fn new() -> Self {
        Self {}
    }
}
