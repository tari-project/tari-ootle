//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct UtxoTagByte(u8);

impl UtxoTagByte {
    pub const fn new(tag: u8) -> Self {
        Self(tag)
    }

    pub const fn as_byte(&self) -> u8 {
        self.0
    }
}
