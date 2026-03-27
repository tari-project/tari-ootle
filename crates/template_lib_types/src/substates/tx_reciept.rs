//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::{BorTag, Deserialize, Serialize, Tagged};
use tari_template_abi::rust::{fmt, str::FromStr};

use crate::{BinaryTag, KeyParseError, ObjectKey, address_prefixes};

const TAG: u64 = BinaryTag::TransactionReceipt.as_u64();

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize, borsh::BorshDeserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionReceiptAddress(#[cfg_attr(feature = "ts", ts(type = "string"))] BorTag<ObjectKey, TAG>);

impl Tagged for TransactionReceiptAddress {
    const TAG: u64 = TAG;
}

impl TransactionReceiptAddress {
    pub const fn from_hash(hash: crate::Hash32) -> Self {
        Self::from_array(hash.into_array())
    }

    pub const fn from_array(arr: [u8; ObjectKey::LENGTH]) -> Self {
        let key = ObjectKey::from_array(arr);
        Self(BorTag::new(key))
    }

    pub const fn as_object_key(&self) -> &ObjectKey {
        self.0.inner()
    }

    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        Ok(Self(BorTag::new(ObjectKey::from_hex(hex)?)))
    }
}

impl<T: Into<crate::Hash32>> From<T> for TransactionReceiptAddress {
    fn from(address: T) -> Self {
        Self::from_hash(address.into())
    }
}

impl fmt::Display for TransactionReceiptAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", address_prefixes::TRANSACTION_RECEIPT, self.as_object_key())
    }
}

impl FromStr for TransactionReceiptAddress {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("txreceipt_").unwrap_or(s);
        Self::from_hex(s)
    }
}
