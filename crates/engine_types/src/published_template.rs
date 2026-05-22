// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    fmt::{Display, Formatter},
    str::FromStr,
};

use tari_bor::{BorTag, Deserialize, Serialize, Tagged};
use tari_ootle_template_metadata::MetadataHash;
use tari_template_lib::types::{
    BinaryTag,
    Hash32,
    KeyParseError,
    MaxBytes,
    MaxString,
    ObjectKey,
    TemplateAddress,
    address_prefixes,
    crypto::RistrettoPublicKeyBytes,
    newtype_struct_serde_impl,
};

use crate::{
    hashing::{hash_template_code, template_hasher32},
    limits,
};

/// Lightweight template metadata that can be exchanged without transmitting the full WASM binary.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PublishedTemplateMetadata {
    /// Human-readable template name extracted from the WASM ABI.
    #[n(0)]
    pub template_name: String,
    /// Author's public key.
    #[n(1)]
    pub author_public_key: RistrettoPublicKeyBytes,
    /// SHA-256 hash of the WASM binary.
    #[n(2)]
    pub binary_hash: Hash32,
    /// Epoch at which the template was published.
    #[n(3)]
    pub at_epoch: u64,
    /// The author-provided off-chain metadata hash
    #[n(4)]
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub metadata_hash: Option<MetadataHash>,
}

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
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
)]
#[cbor(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PublishedTemplateAddress(
    #[n(0)]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    BorTag<ObjectKey, TAG>,
);

impl Tagged for PublishedTemplateAddress {
    const TAG: u64 = TAG;
}

impl PublishedTemplateAddress {
    pub const fn from_hash(hash: Hash32) -> Self {
        let key = ObjectKey::from_array(hash.into_array());
        Self(BorTag::new(key))
    }

    pub fn from_author_and_binary_hash(author_public_key: &RistrettoPublicKeyBytes, binary_hash: &Hash32) -> Self {
        let hash = template_hasher32().chain(author_public_key).chain(binary_hash).result();
        Self::from_hash(hash)
    }

    pub fn from_hex(hex: &str) -> Result<Self, KeyParseError> {
        Ok(Self(BorTag::new(ObjectKey::from_hex(hex)?)))
    }

    pub fn from_template_address(address: TemplateAddress) -> Self {
        Self::from_hash(address)
    }

    pub const fn as_object_key(&self) -> &ObjectKey {
        self.0.inner()
    }

    pub fn as_hash(&self) -> Hash32 {
        Hash32::from_array(self.as_object_key().into_array())
    }

    pub fn as_template_address(&self) -> TemplateAddress {
        self.as_hash()
    }
}

impl<T: Into<Hash32>> From<T> for PublishedTemplateAddress {
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

impl AsRef<[u8]> for PublishedTemplateAddress {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

newtype_struct_serde_impl!(PublishedTemplateAddress, BorTag<ObjectKey, TAG>);

pub type TemplateBlob = MaxBytes<{ limits::ENGINE_LIMITS.max_template_binary_size_bytes }>;

pub type TemplateName = MaxString<{ limits::ENGINE_LIMITS.max_template_name_length }>;

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PublishedTemplate {
    /// Human-readable template name extracted from the WASM ABI.
    #[n(0)]
    #[cbor(default)]
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub template_name: TemplateName,
    /// Author's public key
    #[n(1)]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub author: RistrettoPublicKeyBytes,
    /// Binary of the template
    #[n(2)]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub binary: TemplateBlob,
    /// Epoch at which the template was published
    #[n(3)]
    pub at_epoch: u64,
    /// Optional multihash of off-chain CBOR metadata
    #[n(4)]
    #[cbor(default)]
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub metadata_hash: Option<MetadataHash>,
}

impl PublishedTemplate {
    pub fn to_binary_hash(&self) -> Hash32 {
        hash_template_code(&self.binary)
    }

    pub fn into_template_metadata(self) -> PublishedTemplateMetadata {
        let binary_hash = self.to_binary_hash();
        PublishedTemplateMetadata {
            template_name: self.template_name.into_string(),
            author_public_key: self.author,
            binary_hash,
            at_epoch: self.at_epoch,
            metadata_hash: self.metadata_hash,
        }
    }
}
