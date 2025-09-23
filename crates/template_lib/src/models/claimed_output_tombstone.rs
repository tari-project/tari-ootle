//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use tari_bor::BorTag;
use tari_template_lib_types::{crypto::PedersenCommitmentBytes, KeyParseError, ObjectKey};

use crate::models::{address_prefixes, BinaryTag};

const TAG: u64 = BinaryTag::ClaimedOutputTombstoneAddress.as_u64();

/// The global identifier of a claimed layer-one output in the Tari network.
/// This substate is created when a L1 UTXO is claimed to prevent double claims.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize, borsh::BorshDeserialize))]
pub struct ClaimedOutputTombstoneAddress(#[cfg_attr(feature = "ts", ts(type = "string"))] BorTag<ObjectKey, TAG>);

impl ClaimedOutputTombstoneAddress {
    pub fn new(key: ObjectKey) -> Self {
        Self(BorTag::new(key))
    }

    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        Ok(Self(BorTag::new(ObjectKey::from_hex(hex)?)))
    }

    pub fn from_commitment(commitment_bytes: PedersenCommitmentBytes) -> Self {
        Self(BorTag::new(ObjectKey::from_array(commitment_bytes.into_array())))
    }

    pub fn as_object_key(&self) -> &ObjectKey {
        &self.0
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KeyParseError> {
        Ok(Self(BorTag::new(ObjectKey::try_from(bytes)?)))
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.inner()
    }
}

impl From<ObjectKey> for ClaimedOutputTombstoneAddress {
    fn from(key: ObjectKey) -> Self {
        Self(BorTag::new(key))
    }
}

impl TryFrom<&[u8]> for ClaimedOutputTombstoneAddress {
    type Error = KeyParseError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(value)
    }
}

impl From<[u8; ObjectKey::LENGTH]> for ClaimedOutputTombstoneAddress {
    fn from(value: [u8; ObjectKey::LENGTH]) -> Self {
        Self(BorTag::new(ObjectKey::from_array(value)))
    }
}

impl Display for ClaimedOutputTombstoneAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_{}", address_prefixes::CLAIMED_OUTPUT_TOMBSTONE, self.0.inner())
    }
}

impl FromStr for ClaimedOutputTombstoneAddress {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("tombstone_").unwrap_or(s);
        Self::from_hex(s)
    }
}

impl AsRef<[u8]> for ClaimedOutputTombstoneAddress {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}
