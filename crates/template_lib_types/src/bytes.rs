//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::ops::{Deref, DerefMut};

use crate::serde_helpers::BytesVisitor;

/// A wrapper around `Vec<u8>` that serializes to and from bytes efficiently.
/// Unfortunately, due to some factors (impl serde for Vec<T>, no specialization) ciborium will represent bytes as
/// `Array(vec![Integer(u8), ....])` instead of `Bytes(vec![u8, ...])`. which results in a significant bloat in size.
/// This wrapper uses of the `Bytes` variant in ciborium (similar to `serde_as(as = "Bytes")`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize, borsh::BorshDeserialize))]
#[serde(transparent)]
pub struct Bytes(#[serde(with = "self")] Box<[u8]>);

impl Bytes {
    pub fn from_vec(data: Vec<u8>) -> Self {
        Self(data.into_boxed_slice())
    }
}

impl Deref for Bytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Bytes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(value: Vec<u8>) -> Self {
        Self(value.into_boxed_slice())
    }
}

impl From<Bytes> for Vec<u8> {
    fn from(value: Bytes) -> Self {
        value.0.into_vec()
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

pub fn serialize<S: serde::Serializer, T: AsRef<[u8]>>(v: &T, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_bytes(v.as_ref())
}

pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: From<Box<[u8]>>,
{
    let bytes = d.deserialize_byte_buf(BytesVisitor::new())?;
    Ok(bytes.into_owned().into())
}
