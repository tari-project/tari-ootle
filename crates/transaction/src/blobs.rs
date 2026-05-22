//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use serde::{Deserialize, Serialize};
use tari_engine_types::hashing::{EngineHashDomainLabel, hasher32};
use tari_template_lib_types::{Hash32, MaxBytes, bytes::Bytes};

/// Index of a blob within a transaction's blob list. `u8` keeps argument references cheap and
/// caps any single transaction at 256 blobs — well above realistic per-transaction needs.
pub type BlobIndex = u8;

/// Returned when a transaction would exceed the maximum number of blobs (`u8::MAX + 1`).
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone, Copy)]
#[error("transaction blob count would exceed the BlobIndex range ({max} blobs maximum)")]
pub struct BlobIndexOverflow {
    pub max: usize,
}

/// An opaque, content-addressed payload referenced from a transaction's instructions by index.
///
/// Blobs are the prunable side-channel of a transaction: the bytes themselves are not bound
/// directly into the signing domain — only their per-blob hashes are. Once a transaction has
/// been finalized and its retention window has elapsed, blob payloads can be discarded without
/// affecting the transaction id or signature verifiability.
///
/// JSON encoding is base64 (matches `PublishTemplate` binary today). Borsh and CBOR use the
/// `Bytes` byte-array form.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "string"))]
pub struct Blob(
    #[n(0)]
    #[serde(
        serialize_with = "ootle_serde::base64::serialize",
        deserialize_with = "ootle_serde::base64::deserialize"
    )]
    Bytes,
);

impl Blob {
    pub fn new<B: Into<Bytes>>(bytes: B) -> Self {
        Self(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the per-blob commitment used in the transaction signing domain.
    pub fn hash(&self) -> Hash32 {
        hash_blob(self)
    }
}

impl Deref for Blob {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.0.as_slice()
    }
}

impl From<Vec<u8>> for Blob {
    fn from(bytes: Vec<u8>) -> Self {
        Self(Bytes::from(bytes))
    }
}

impl From<Bytes> for Blob {
    fn from(bytes: Bytes) -> Self {
        Self(bytes)
    }
}

impl<const N: usize> From<MaxBytes<N>> for Blob {
    fn from(bytes: MaxBytes<N>) -> Self {
        Self(Bytes::from(bytes.into_vec()))
    }
}

impl From<Box<[u8]>> for Blob {
    fn from(bytes: Box<[u8]>) -> Self {
        Self(Bytes::from_vec(bytes.into_vec()))
    }
}

/// Domain-separated per-blob hash. The `Blob` label keeps blob bytes in a distinct domain from
/// any signature, id, or substate hashing context.
pub fn hash_blob(blob: &Blob) -> Hash32 {
    hasher32(EngineHashDomainLabel::Blob).chain(&blob.as_bytes()).result()
}

/// Ordered collection of `Blob`s carried by a transaction. Index 0 corresponds to the first
/// blob, etc. Instructions and arguments reference blobs by `BlobIndex` into this list.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Blobs(#[n(0)] Vec<Blob>);

impl Blobs {
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    pub fn from_vec(blobs: Vec<Blob>) -> Self {
        Self(blobs)
    }

    /// Append a blob and return its assigned `BlobIndex`.
    ///
    /// Returns `Err` if the blob count would exceed the `BlobIndex` range.
    pub fn push(&mut self, blob: Blob) -> Result<BlobIndex, BlobIndexOverflow> {
        let max = BlobIndex::MAX as usize + 1;
        if self.0.len() >= max {
            return Err(BlobIndexOverflow { max });
        }
        let idx = self.0.len() as BlobIndex;
        self.0.push(blob);
        Ok(idx)
    }

    pub fn get(&self, idx: BlobIndex) -> Option<&Blob> {
        self.0.get(idx as usize)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Blob> + '_ {
        self.0.iter()
    }

    pub fn as_slice(&self) -> &[Blob] {
        &self.0
    }

    /// Compute the per-blob commitment collection. Same `Blobs` always produces the same
    /// `BlobHashes`. This is the only public way to obtain a `BlobHashes`.
    pub fn hashes(&self) -> BlobHashes {
        BlobHashes(self.0.iter().map(hash_blob).collect())
    }
}

/// A typed collection of per-blob commitments — the prunable surrogate of `Blobs` in the
/// signing domain.
///
/// The only public way to obtain a non-default `BlobHashes` is via `Blobs::hashes()`. There is
/// no `from_raw` constructor: anywhere a `BlobHashes` appears, it either originated from
/// hashing real blobs or arrived via deserialization of bytes previously written by this
/// crate's storage layer (and is bound by a signature, so tampering is detectable).
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BlobHashes(#[n(0)] Vec<Hash32>);

impl BlobHashes {
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    pub fn as_slice(&self) -> &[Hash32] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use borsh::BorshSerialize;

    use super::*;

    #[test]
    fn hash_is_deterministic() {
        let b = Blob::from(vec![1, 2, 3, 4]);
        assert_eq!(b.hash(), b.hash());
    }

    #[test]
    fn hash_distinguishes_contents() {
        let a = Blob::from(vec![1, 2, 3]);
        let b = Blob::from(vec![1, 2, 4]);
        assert_ne!(a.hash(), b.hash());
    }

    #[test]
    fn empty_blob_has_a_hash() {
        let b = Blob::default();
        assert_eq!(b.hash(), b.hash());
    }

    #[test]
    fn blobs_push_assigns_sequential_indices() {
        let mut blobs = Blobs::empty();
        let i0 = blobs.push(Blob::from(vec![0])).unwrap();
        let i1 = blobs.push(Blob::from(vec![1])).unwrap();
        let i2 = blobs.push(Blob::from(vec![2])).unwrap();
        assert_eq!(i0, 0);
        assert_eq!(i1, 1);
        assert_eq!(i2, 2);
        assert_eq!(blobs.len(), 3);
        assert_eq!(blobs.get(1).map(|b| b.as_bytes()), Some(&[1u8][..]));
    }

    #[test]
    fn blobs_push_rejects_overflow() {
        let mut blobs = Blobs::empty();
        for _ in 0..=u8::MAX as usize {
            blobs.push(Blob::default()).unwrap();
        }
        assert_eq!(blobs.len(), u8::MAX as usize + 1);
        // 257th push must fail
        assert!(blobs.push(Blob::default()).is_err());
    }

    #[test]
    fn hashes_match_per_blob_hash_in_order() {
        let blobs = Blobs::from_vec(vec![
            Blob::from(vec![1]),
            Blob::from(vec![2, 2]),
            Blob::from(vec![3, 3, 3]),
        ]);
        let hashes = blobs.hashes();
        assert_eq!(hashes.len(), 3);
        assert_eq!(hashes.as_slice()[0], blobs.get(0).unwrap().hash());
        assert_eq!(hashes.as_slice()[1], blobs.get(1).unwrap().hash());
        assert_eq!(hashes.as_slice()[2], blobs.get(2).unwrap().hash());
    }

    #[test]
    fn hashes_change_when_order_changes() {
        let a = Blobs::from_vec(vec![Blob::from(vec![1]), Blob::from(vec![2])]);
        let b = Blobs::from_vec(vec![Blob::from(vec![2]), Blob::from(vec![1])]);
        assert_ne!(a.hashes(), b.hashes());
    }

    #[test]
    fn blob_hashes_borsh_roundtrip() {
        let hashes = Blobs::from_vec(vec![Blob::from(vec![1, 2, 3])]).hashes();
        let mut buf = Vec::new();
        BorshSerialize::serialize(&hashes, &mut buf).unwrap();
        // Length-prefix + each Hash32 (32 bytes). u32 little-endian length prefix => 4 + 32 = 36.
        assert_eq!(buf.len(), 4 + 32);
    }

    #[test]
    fn blob_serde_json_roundtrip() {
        let blob = Blob::from(vec![1, 2, 3, 4]);
        let json = serde_json::to_string(&blob).unwrap();
        let decoded: Blob = serde_json::from_str(&json).unwrap();
        assert_eq!(blob, decoded);
    }

    #[test]
    fn blobs_serde_json_roundtrip() {
        let blobs = Blobs::from_vec(vec![Blob::from(vec![1]), Blob::from(vec![2, 3])]);
        let json = serde_json::to_string(&blobs).unwrap();
        let decoded: Blobs = serde_json::from_str(&json).unwrap();
        assert_eq!(blobs, decoded);
    }
}
