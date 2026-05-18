//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::fmt;

/// A 4-byte tag associated with a UTXO. This can be used to more quickly identify _possible_ ownership of a UTXO
/// without needing to fully download and verify all UTXOs.
/// 4 bytes gives a reasonable trade-off between initial download size and utxo size.
#[derive(Debug, Clone, Copy, Encode, Decode, CborLen, Hash, PartialEq, Eq)]
#[cbor(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct UtxoTag(#[n(0)] u32);

impl UtxoTag {
    pub const fn new(tag: u32) -> Self {
        Self(tag)
    }

    pub const fn value(&self) -> u32 {
        self.0
    }
}

impl From<u32> for UtxoTag {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl fmt::Display for UtxoTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UtxoTag({})", self.0)
    }
}
