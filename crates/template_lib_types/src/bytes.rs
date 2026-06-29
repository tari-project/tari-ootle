//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::{
    ops::{Deref, DerefMut},
    prelude::*,
};

#[cfg(feature = "serde")]
use crate::serde_helpers::BytesVisitor;

/// A wrapper around a byte buffer that encodes as CBOR major type 2 (Bytes).
///
/// Without this wrapper, deriving `Encode`/`Decode` on a `Vec<u8>` field would serialise as
/// `Array(Integer(u8), ...)`, which is much larger than the dedicated `Bytes` representation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize, borsh::BorshDeserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "Uint8Array"))]
#[cfg_attr(feature = "std", derive(Hash))]
pub struct Bytes(#[cfg_attr(feature = "serde", serde(with = "self"))] Box<[u8]>);

impl<C> minicbor::Encode<C> for Bytes {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.bytes(&self.0)?;
        Ok(())
    }
}

impl<'b, C> minicbor::Decode<'b, C> for Bytes {
    fn decode(d: &mut minicbor::Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        Ok(Self(bytes.to_vec().into_boxed_slice()))
    }
}

impl<C> minicbor::CborLen<C> for Bytes {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        minicbor::bytes::cbor_len(self.0.as_ref(), ctx)
    }
}

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
#[cfg(feature = "serde")]
pub fn serialize<S: serde::Serializer, T: AsRef<[u8]>>(v: &T, s: S) -> Result<S::Ok, S::Error> {
    use crate::hex::bytes_to_hex;
    if s.is_human_readable() {
        let st = bytes_to_hex(v.as_ref());
        s.serialize_str(&st)
    } else {
        s.serialize_bytes(v.as_ref())
    }
}

#[cfg(feature = "serde")]
pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: From<Box<[u8]>>,
{
    use serde::de::{Deserialize, Error};

    use crate::hex::bytes_from_hex;
    if d.is_human_readable() {
        let hex = Box::<str>::deserialize(d)?;
        let bytes = bytes_from_hex(&hex).map_err(Error::custom)?;
        Ok(bytes.into_boxed_slice().into())
    } else {
        let bytes = d.deserialize_byte_buf(BytesVisitor::new())?;
        Ok(bytes.into_owned().into())
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use super::*;

    #[test]
    fn encode_decode_cbor() {
        let original = Bytes::from_vec(vec![1, 2, 3, 4, 5]);
        let val = tari_bor::to_value(&original).unwrap();
        let arr = val.as_bytes().expect("Expected bytes");
        let deserialized: Bytes = tari_bor::from_value(&val).unwrap();
        assert_eq!(original, deserialized);
        assert_eq!(arr, original.as_slice());

        // The CBOR wire form must be a byte string (major type 2 = `0x40..=0x5f`), never an array of integers
        // (major type 4). The top 3 bits of the head byte are the major type.
        let raw = tari_bor::encode(&original).unwrap();
        assert_eq!(
            raw[0] >> 5,
            2,
            "Bytes must CBOR-encode as a byte string, got head byte 0x{:02x}",
            raw[0]
        );
    }

    // Regression: a self-describing format (JSON) has no native byte type, so `serialize_bytes` is rendered as an array
    // of integers. The deserializer must read that array back, or any `Bytes`-bearing value (e.g. a stored transaction
    // with a script-path witness) fails to decode.
    #[cfg(feature = "serde")]
    #[test]
    fn json_round_trips() {
        let original = Bytes::from_vec(vec![0, 1, 2, 250, 255]);
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.starts_with('['), "expected a JSON array, got {json}");
        let deserialized: Bytes = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    // A value decoded from JSON (an integer array) must re-encode to CBOR as the canonical byte string — identical to
    // the original's CBOR — so a JSON -> CBOR round-trip (e.g. an indexer recomputing a transaction hash) cannot turn a
    // byte string into `Array(int, int, ...)` and change the bytes.
    #[cfg(feature = "serde")]
    #[test]
    fn json_then_cbor_is_the_canonical_byte_string() {
        let original = Bytes::from_vec(vec![1, 2, 3, 4, 5]);
        let from_json: Bytes = serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();

        let cbor_from_json = tari_bor::encode(&from_json).unwrap();
        assert_eq!(cbor_from_json, tari_bor::encode(&original).unwrap());
        assert_eq!(
            cbor_from_json[0] >> 5,
            2,
            "JSON -> CBOR must yield a byte string, not an array"
        );
    }
}
