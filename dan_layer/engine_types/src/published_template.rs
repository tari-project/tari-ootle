// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    fmt::{Display, Formatter},
    str::FromStr,
};

use tari_bor::{BorTag, Deserialize, Serialize};
use tari_template_lib::{
    models::{BinaryTag, KeyParseError, ObjectKey},
    Hash,
};

const TAG: u64 = BinaryTag::TemplateAddress.as_u64();

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct PublishedTemplateAddress(#[cfg_attr(feature = "ts", ts(type = "string"))] BorTag<ObjectKey, TAG>);

impl PublishedTemplateAddress {
    pub const fn from_hash(hash: Hash) -> Self {
        let key = ObjectKey::from_array(hash.into_array());
        Self(BorTag::new(key))
    }

    pub fn as_object_key(&self) -> &ObjectKey {
        self.0.inner()
    }

    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        Ok(Self(BorTag::new(ObjectKey::from_hex(hex)?)))
    }

    pub fn as_hash(&self) -> Hash {
        Hash::from_array(self.as_object_key().into_array())
    }
}

impl<T: Into<Hash>> From<T> for PublishedTemplateAddress {
    fn from(address: T) -> Self {
        Self::from_hash(address.into())
    }
}

impl Display for PublishedTemplateAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "template_{}", self.as_object_key())
    }
}

impl FromStr for PublishedTemplateAddress {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("template_").unwrap_or(s);
        Self::from_hex(s)
    }
}

impl borsh::BorshSerialize for PublishedTemplateAddress {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        borsh::BorshSerialize::serialize(self.as_object_key().array(), writer)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct PublishedTemplate {
    pub binary: Vec<u8>,
}
