//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_abi::rust::fmt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoTagByte(u8);

impl UtxoTagByte {
    pub const fn new(tag: u8) -> Self {
        Self(tag)
    }

    pub const fn as_byte(&self) -> u8 {
        self.0
    }
}

impl fmt::Display for UtxoTagByte {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TagByte({})", self.0)
    }
}
