//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_abi::rust::fmt;

/// A 4-byte tag associated with a UTXO. This can be used to more quickly identify _possible_ ownership of a UTXO
/// without needing to fully download and verify all UTXOs.
/// 4 bytes gives a reasonable trade-off between initial download size and utxo size.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoTag(u32);

impl UtxoTag {
    pub const fn new(tag: u32) -> Self {
        Self(tag)
    }

    pub const fn value(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for UtxoTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UtxoTag({})", self.0)
    }
}
