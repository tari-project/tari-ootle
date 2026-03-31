//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use multihash::Multihash;
use serde::{Deserialize, Serialize, de};
use sha2::{Digest, Sha256};

/// Multihash function code for SHA-256 (IPFS CIDv1 default).
const SHA2_256_CODE: u64 = 0x12;

/// Maximum size of a metadata multihash digest in bytes.
/// This allows SHA-512 (64 bytes) and is also the IPFS CID default size.
pub const MAX_DIGEST_SIZE: usize = 64;

/// Multihash-encoded hash of template metadata CBOR.
///
/// Stack-allocated via [`multihash::Multihash`] — no heap allocation.
/// Maximum digest size is [`MAX_DIGEST_SIZE`] bytes.
///
/// Default hash function: SHA-256 (code 0x12), compatible with IPFS CIDv1.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MetadataHash(Multihash<MAX_DIGEST_SIZE>);

impl MetadataHash {
    /// Create a MetadataHash from a raw multihash byte slice.
    /// Returns `None` if the bytes are not a valid multihash or exceed the digest size limit.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Multihash::from_bytes(bytes).ok().map(Self)
    }

    /// Compute a SHA-256 multihash of the given data.
    pub fn hash_sha256(data: &[u8]) -> Self {
        let digest = Sha256::digest(data);
        Self(Multihash::wrap(SHA2_256_CODE, &digest).expect("SHA-256 digest fits in 64 bytes"))
    }

    /// Verify that this hash matches the given data.
    /// Returns an error if the hash function is unsupported.
    pub fn verify(&self, data: &[u8]) -> Result<bool, MetadataHashError> {
        match self.0.code() {
            SHA2_256_CODE => {
                let expected = Self::hash_sha256(data);
                Ok(self == &expected)
            },
            code => Err(MetadataHashError::UnsupportedHashFunction(code)),
        }
    }

    /// Return the raw multihash-encoded bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_bytes()
    }

    /// Return the multihash bytes as a hex string.
    pub fn to_hex(&self) -> String {
        hex_encode(&self.0.to_bytes())
    }

    /// Parse a MetadataHash from a hex string.
    pub fn from_hex(hex: &str) -> Result<Self, MetadataHashError> {
        let bytes = hex_decode(hex).map_err(|_| MetadataHashError::InvalidHex)?;
        Self::from_bytes(&bytes).ok_or(MetadataHashError::InvalidMultihash)
    }

    /// Return the inner `Multihash`.
    pub fn into_inner(self) -> Multihash<MAX_DIGEST_SIZE> {
        self.0
    }

    /// Return a reference to the inner `Multihash`.
    pub fn as_multihash(&self) -> &Multihash<MAX_DIGEST_SIZE> {
        &self.0
    }
}

impl From<Multihash<MAX_DIGEST_SIZE>> for MetadataHash {
    fn from(mh: Multihash<MAX_DIGEST_SIZE>) -> Self {
        Self(mh)
    }
}

impl fmt::Debug for MetadataHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MetadataHash({})", self.to_hex())
    }
}

impl fmt::Display for MetadataHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl Serialize for MetadataHash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for MetadataHash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let hex = String::deserialize(deserializer)?;
        MetadataHash::from_hex(&hex).map_err(de::Error::custom)
    }
}

#[cfg(feature = "borsh")]
impl borsh::BorshSerialize for MetadataHash {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let bytes = self.0.to_bytes();
        borsh::BorshSerialize::serialize(&bytes, writer)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MetadataHashError {
    #[error("Unsupported hash function code: 0x{0:02x}")]
    UnsupportedHashFunction(u64),
    #[error("Invalid hex string")]
    InvalidHex,
    #[error("Invalid multihash encoding")]
    InvalidMultihash,
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, ()> {
    if !hex.len().is_multiple_of(2) {
        return Err(());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| ()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_multihash_roundtrip() {
        let data = b"hello world";
        let hash = MetadataHash::hash_sha256(data);

        // SHA-256 multihash: code=0x12, size=0x20 (32 bytes), then 32 bytes digest
        let bytes = hash.to_bytes();
        assert_eq!(bytes[0], 0x12);
        assert_eq!(bytes[1], 32);
        assert_eq!(bytes.len(), 34);

        assert!(hash.verify(data).unwrap());
        assert!(!hash.verify(b"different data").unwrap());
    }

    #[test]
    fn stack_allocated() {
        // Multihash<64> is stack-allocated: 64 bytes buf + 8 bytes code + 1 byte len (approx)
        let hash = MetadataHash::hash_sha256(b"test");
        // Should be no larger than the Multihash struct itself
        assert!(std::mem::size_of_val(&hash) <= std::mem::size_of::<Multihash<MAX_DIGEST_SIZE>>());
    }

    #[test]
    fn from_bytes_rejects_invalid() {
        // Too short
        assert!(MetadataHash::from_bytes(&[0x12]).is_none());
        // Valid SHA-256 multihash should work
        let hash = MetadataHash::hash_sha256(b"test");
        let bytes = hash.to_bytes();
        assert!(MetadataHash::from_bytes(&bytes).is_some());
    }

    #[test]
    fn hex_roundtrip() {
        let hash = MetadataHash::hash_sha256(b"test");
        let hex = hash.to_hex();
        let parsed = MetadataHash::from_hex(&hex).unwrap();
        assert_eq!(hash, parsed);
    }

    #[test]
    fn json_serde_roundtrip() {
        let hash = MetadataHash::hash_sha256(b"test");
        let json = serde_json::to_string(&hash).unwrap();
        let parsed: MetadataHash = serde_json::from_str(&json).unwrap();
        assert_eq!(hash, parsed);
    }

    #[test]
    fn cbor_serde_roundtrip() {
        let hash = MetadataHash::hash_sha256(b"test");
        let cbor = tari_bor::encode(&hash).unwrap();
        let parsed: MetadataHash = tari_bor::decode(&cbor).unwrap();
        assert_eq!(hash, parsed);
    }

    #[test]
    fn to_bytes_roundtrip() {
        let hash = MetadataHash::hash_sha256(b"test");
        let bytes = hash.to_bytes();
        let parsed = MetadataHash::from_bytes(&bytes).unwrap();
        assert_eq!(hash, parsed);
    }
}
