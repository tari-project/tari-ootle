//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::{BorTag, Deserialize, Serialize};
use tari_template_abi::rust::{fmt, str::FromStr};

use crate::{BinaryTag, Hash32, KeyParseError, ObjectKey, address_prefixes};

const TAG: u64 = BinaryTag::ValidatorNodeFeePool.as_u64();

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize, borsh::BorshDeserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ValidatorFeePoolAddress(#[cfg_attr(feature = "ts", ts(type = "string"))] BorTag<ObjectKey, TAG>);

impl ValidatorFeePoolAddress {
    pub const fn from_array(arr: [u8; ObjectKey::LENGTH]) -> Self {
        let key = ObjectKey::from_array(arr);
        Self(BorTag::new(key))
    }

    pub const fn as_object_key(&self) -> &ObjectKey {
        self.0.inner()
    }

    pub const fn as_slice(&self) -> &[u8] {
        self.0.inner().array()
    }

    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        Ok(Self(BorTag::new(ObjectKey::from_hex(hex)?)))
    }

    pub fn as_hash(&self) -> Hash32 {
        Hash32::from_array(self.as_object_key().into_array())
    }
}

impl From<[u8; 32]> for ValidatorFeePoolAddress {
    fn from(arr: [u8; 32]) -> Self {
        Self::from_array(arr)
    }
}

impl fmt::Display for ValidatorFeePoolAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", address_prefixes::VALIDATOR_FEE_POOL, self.as_object_key())
    }
}

impl TryFrom<&[u8]> for ValidatorFeePoolAddress {
    type Error = KeyParseError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != ObjectKey::LENGTH {
            return Err(KeyParseError);
        }

        let mut key = [0u8; ObjectKey::LENGTH];
        key.copy_from_slice(value);
        Ok(Self::from_array(key))
    }
}

impl FromStr for ValidatorFeePoolAddress {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("vnfp_").unwrap_or(s);
        Self::from_hex(s)
    }
}
