//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::ops::{Deref, DerefMut};

use crate::serde_helpers::BytesVisitor;

/// A wrapper around a byte buffer that implements efficient serde serialisation.
///
/// Unfortunately, because we cannot implement a specialized version of serde::Serialize (impl serde::Serialize for
/// Vec<u8>) ciborium will represent bytes as `Array(vec![Integer(u8), ....])` instead of `Bytes(vec![u8, ...])`. which
/// results in a significant size overhead. This wrapper uses the `Value::Bytes` variant (similar to `serde_as(as =
/// "Bytes")`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize, borsh::BorshDeserialize))]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "Uint8Array"))]
#[cfg_attr(feature = "std", derive(Hash))]
pub struct Bytes(#[serde(with = "self")] Box<[u8]>);

impl Bytes {
    pub fn from_vec(data: Vec<u8>) -> Self {
        Self(data.into_boxed_slice())
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.0.into_vec()
    }

    pub fn into_boxed(self) -> Box<[u8]> {
        self.0
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
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

impl From<Bytes> for Box<[u8]> {
    fn from(value: Bytes) -> Self {
        value.0
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Serialize using the optimal byte format. i.e. `Bytes` in ciborium instead of `Array(Integer(u8), ....])`
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_cbor() {
        let original = Bytes::from_vec(vec![1, 2, 3, 4, 5]);
        let val = tari_bor::to_value(&original).unwrap();
        let arr = val.as_bytes().expect("Expected bytes");
        let deserialized: Bytes = tari_bor::from_value(&val).unwrap();
        assert_eq!(original, deserialized);
        assert_eq!(arr, original.as_slice());
    }
}
