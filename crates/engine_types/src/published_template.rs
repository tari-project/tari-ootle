// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    fmt::{Display, Formatter},
    str::FromStr,
};

use tari_bor::{BorTag, Deserialize, Serialize};
use tari_template_lib::{
    models::{address_prefixes, BinaryTag},
    types::{crypto::RistrettoPublicKeyBytes, Hash, KeyParseError, ObjectKey, TemplateAddress},
};

use crate::hashing::{hasher32, EngineHashDomainLabel};

const TAG: u64 = BinaryTag::TemplateAddress.as_u64();

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PublishedTemplateAddress(#[cfg_attr(feature = "ts", ts(type = "string"))] BorTag<ObjectKey, TAG>);

impl PublishedTemplateAddress {
    pub const fn from_hash(hash: Hash) -> Self {
        let key = ObjectKey::from_array(hash.into_array());
        Self(BorTag::new(key))
    }

    pub fn from_author_and_binary_hash(author_public_key: &RistrettoPublicKeyBytes, binary_hash: &Hash) -> Self {
        let hash = hasher32(EngineHashDomainLabel::TemplateAddress)
            .chain(author_public_key)
            .chain(binary_hash)
            .result();
        Self::from_hash(hash)
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

    pub fn as_template_address(&self) -> TemplateAddress {
        self.as_hash()
    }
}

impl<T: Into<Hash>> From<T> for PublishedTemplateAddress {
    fn from(address: T) -> Self {
        Self::from_hash(address.into())
    }
}

impl Display for PublishedTemplateAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", address_prefixes::TEMPLATE, self.as_object_key())
    }
}

impl FromStr for PublishedTemplateAddress {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("template_").unwrap_or(s);
        Self::from_hex(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PublishedTemplate {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub author: RistrettoPublicKeyBytes,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub binary_hash: Hash,
}
