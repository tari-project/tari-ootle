//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_template_lib_types::TemplateAddress;
use url::Url;

use crate::{MetadataHash, MetadataHashWriter};

/// Current schema version for template metadata.
pub const SCHEMA_VERSION: u32 = 1;

/// Off-chain template metadata, serialized as CBOR for hashing and verification.
///
/// Template authors populate these fields via their `Cargo.toml` `[package]` and
/// `[package.metadata.tari-template]` sections. The CBOR encoding of this struct is
/// hashed to produce a [`MetadataHash`] that is stored on-chain alongside the template binary.
#[derive(Debug, Clone, Encode, Decode, CborLen, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TemplateMetadata {
    #[n(0)]
    pub schema_version: u32,
    #[n(1)]
    pub name: String,
    #[n(2)]
    pub version: String,
    #[n(3)]
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[n(4)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[n(5)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[n(6)]
    #[cbor(with = "option_url")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub repository: Option<Url>,
    /// The commit hash of the source code used to build this template, for reproducible build verification.
    #[n(7)]
    #[cbor(with = "option_oid")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "{ Sha1: string } | null"))]
    pub commit_hash: Option<gix_hash::ObjectId>,
    #[n(8)]
    #[cbor(with = "option_url")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub documentation: Option<Url>,
    #[n(9)]
    #[cbor(with = "option_url")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub homepage: Option<Url>,
    #[n(10)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[n(11)]
    #[cbor(with = "option_url")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub logo_url: Option<Url>,
    /// The template address of a previous version that this template supersedes.
    #[n(12)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub supersedes: Option<TemplateAddress>,
    /// Rustdoc comments extracted from public functions of the template, keyed by source order.
    #[n(13)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<FunctionDoc>,
    #[n(14)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

/// Off-chain documentation for a single public template function.
#[derive(Debug, Clone, Encode, Decode, CborLen, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FunctionDoc {
    #[n(0)]
    pub name: String,
    #[n(1)]
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub doc: String,
}

/// minicbor adapter for `Option<Url>`. Encodes `None` as CBOR null and `Some(url)` as a text string.
mod option_url {
    use minicbor::{CborLen, Decoder, Encoder};
    use url::Url;

    pub fn encode<C, W: minicbor::encode::Write>(
        v: &Option<Url>,
        e: &mut Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        match v {
            None => {
                e.null()?;
            },
            Some(u) => {
                e.str(u.as_str())?;
            },
        }
        Ok(())
    }

    pub fn decode<'b, C>(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Option<Url>, minicbor::decode::Error> {
        if matches!(d.datatype()?, minicbor::data::Type::Null) {
            d.skip()?;
            return Ok(None);
        }
        let s = d.str()?;
        Url::parse(s)
            .map(Some)
            .map_err(|e| minicbor::decode::Error::message(format!("invalid Url: {e}")))
    }

    pub fn cbor_len<C>(v: &Option<Url>, ctx: &mut C) -> usize {
        match v {
            None => <() as CborLen<C>>::cbor_len(&(), ctx),
            Some(u) => <str as CborLen<C>>::cbor_len(u.as_str(), ctx),
        }
    }
}

/// minicbor adapter for `Option<gix_hash::ObjectId>`. Encodes as CBOR bytes (or null).
mod option_oid {
    use gix_hash::ObjectId;
    use minicbor::{CborLen, Decoder, Encoder};

    pub fn encode<C, W: minicbor::encode::Write>(
        v: &Option<ObjectId>,
        e: &mut Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        match v {
            None => {
                e.null()?;
            },
            Some(oid) => {
                e.bytes(oid.as_bytes())?;
            },
        }
        Ok(())
    }

    pub fn decode<'b, C>(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Option<ObjectId>, minicbor::decode::Error> {
        if matches!(d.datatype()?, minicbor::data::Type::Null) {
            d.skip()?;
            return Ok(None);
        }
        let bytes = d.bytes()?;
        ObjectId::try_from(bytes)
            .map(Some)
            .map_err(|e| minicbor::decode::Error::message(format!("invalid ObjectId: {e}")))
    }

    pub fn cbor_len<C>(v: &Option<ObjectId>, ctx: &mut C) -> usize {
        match v {
            None => <() as CborLen<C>>::cbor_len(&(), ctx),
            Some(oid) => <minicbor::bytes::ByteSlice as CborLen<C>>::cbor_len(oid.as_bytes().into(), ctx),
        }
    }
}

impl TemplateMetadata {
    /// Create a new TemplateMetadata with only the required fields.
    pub fn new(name: String, version: String) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            name,
            version,
            description: String::new(),
            tags: Vec::new(),
            category: None,
            repository: None,
            commit_hash: None,
            documentation: None,
            homepage: None,
            license: None,
            logo_url: None,
            supersedes: None,
            functions: Vec::new(),
            extra: BTreeMap::new(),
        }
    }

    /// Encode this metadata as canonical CBOR bytes.
    pub fn to_cbor(&self) -> Result<Vec<u8>, TemplateMetadataError> {
        tari_bor::encode(self).map_err(TemplateMetadataError::CborEncode)
    }

    /// Write CBOR-encoded metadata directly to a writer without intermediate allocation.
    pub fn write_cbor_to<W: std::io::Write>(&self, writer: &mut W) -> Result<(), TemplateMetadataError> {
        tari_bor::encode_into_writer(self, writer).map_err(TemplateMetadataError::CborEncode)
    }

    /// Decode metadata from CBOR bytes.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, TemplateMetadataError> {
        tari_bor::decode(bytes).map_err(TemplateMetadataError::CborDecode)
    }

    /// Serialize this metadata as a JSON string.
    #[cfg(feature = "json")]
    pub fn to_json(&self) -> Result<String, TemplateMetadataError> {
        serde_json::to_string_pretty(self).map_err(TemplateMetadataError::JsonEncode)
    }

    /// Deserialize metadata from a JSON string.
    #[cfg(feature = "json")]
    pub fn from_json(json: &str) -> Result<Self, TemplateMetadataError> {
        serde_json::from_str(json).map_err(TemplateMetadataError::JsonDecode)
    }

    /// Compute the domain-separated SHA-256 multihash of the CBOR-encoded metadata.
    ///
    /// CBOR is written directly into the hasher — no intermediate buffer allocation.
    pub fn hash(&self) -> Result<MetadataHash, TemplateMetadataError> {
        let mut writer = MetadataHashWriter::new();
        self.write_cbor_to(&mut writer)?;
        Ok(writer.finalize())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateMetadataError {
    #[error("CBOR encoding error: {0}")]
    CborEncode(tari_bor::BorError),
    #[error("CBOR decoding error: {0}")]
    CborDecode(tari_bor::BorError),
    #[cfg(feature = "json")]
    #[error("JSON encoding error: {0}")]
    JsonEncode(serde_json::Error),
    #[cfg(feature = "json")]
    #[error("JSON decoding error: {0}")]
    JsonDecode(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cbor_roundtrip() {
        let metadata = TemplateMetadata {
            schema_version: 1,
            name: "test-template".to_string(),
            version: "1.0.0".to_string(),
            description: "A test template".to_string(),
            tags: vec!["test".to_string(), "example".to_string()],
            category: Some("utility".to_string()),
            repository: Some(Url::parse("https://github.com/example/test").unwrap()),
            commit_hash: Some(gix_hash::ObjectId::from_hex(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()),
            documentation: None,
            homepage: None,
            license: Some("BSD-3-Clause".to_string()),
            logo_url: None,
            supersedes: Some(
                TemplateAddress::from_hex("0000000000000000000000000000000000000000000000000000000000000001").unwrap(),
            ),
            functions: vec![FunctionDoc {
                name: "transfer".to_string(),
                doc: "Move funds between accounts.".to_string(),
            }],
            extra: BTreeMap::new(),
        };

        let cbor = metadata.to_cbor().unwrap();
        let decoded = TemplateMetadata::from_cbor(&cbor).unwrap();
        assert_eq!(metadata, decoded);
    }

    #[test]
    #[cfg(feature = "json")]
    fn json_roundtrip() {
        let metadata = TemplateMetadata::new("my-template".to_string(), "0.1.0".to_string());
        let json = metadata.to_json().unwrap();
        let decoded = TemplateMetadata::from_json(&json).unwrap();
        assert_eq!(metadata, decoded);
    }

    #[test]
    fn hash_deterministic() {
        let metadata = TemplateMetadata::new("test".to_string(), "1.0.0".to_string());
        let hash1 = metadata.hash().unwrap();
        let hash2 = metadata.hash().unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn hash_changes_with_content() {
        let m1 = TemplateMetadata::new("a".to_string(), "1.0.0".to_string());
        let m2 = TemplateMetadata::new("b".to_string(), "1.0.0".to_string());
        assert_ne!(m1.hash().unwrap(), m2.hash().unwrap());
    }

    #[test]
    fn cbor_backward_compat_missing_optional_fields() {
        // A CBOR blob with only required fields should deserialize fine
        let minimal = TemplateMetadata::new("test".to_string(), "1.0.0".to_string());
        let cbor = minimal.to_cbor().unwrap();
        let decoded = TemplateMetadata::from_cbor(&cbor).unwrap();
        assert_eq!(decoded.tags, Vec::<String>::new());
        assert_eq!(decoded.category, None);
    }

    #[test]
    fn write_cbor_to_and_from_cbor_roundtrip() {
        let metadata = TemplateMetadata::new("stream-test".to_string(), "2.0.0".to_string());
        let mut buf = Vec::new();
        metadata.write_cbor_to(&mut buf).unwrap();
        let decoded = TemplateMetadata::from_cbor(&buf).unwrap();
        assert_eq!(metadata, decoded);
    }

    #[test]
    fn write_cbor_to_matches_to_cbor() {
        let metadata = TemplateMetadata {
            schema_version: 1,
            name: "consistency-test".to_string(),
            version: "1.0.0".to_string(),
            description: "Check writer produces same bytes".to_string(),
            tags: vec!["a".to_string()],
            category: Some("test".to_string()),
            repository: None,
            commit_hash: None,
            documentation: None,
            homepage: None,
            license: None,
            logo_url: None,
            supersedes: None,
            functions: Vec::new(),
            extra: BTreeMap::new(),
        };

        let allocated = metadata.to_cbor().unwrap();
        let mut streamed = Vec::new();
        metadata.write_cbor_to(&mut streamed).unwrap();
        assert_eq!(allocated, streamed);
    }

    #[test]
    fn read_cbor_from_file() {
        let metadata = TemplateMetadata::new("file-test".to_string(), "0.1.0".to_string());
        let cbor = metadata.to_cbor().unwrap();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut tmp, &cbor).unwrap();

        let bytes = std::fs::read(tmp.path()).unwrap();
        let decoded = TemplateMetadata::from_cbor(&bytes).unwrap();
        assert_eq!(metadata, decoded);
    }
}
