//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ::serde::{
    Deserialize,
    Deserializer,
    Serializer,
    de::{Error, Visitor},
};
use tari_template_abi::rust::{any, fmt, format, marker::PhantomData, prelude::*};

// Cow is not available in no_std, so we define our own
pub enum BytesCow<'a> {
    Borrowed(&'a [u8]),
    Owned(Box<[u8]>),
}

impl<'a> BytesCow<'a> {
    pub const fn len(&self) -> usize {
        match self {
            BytesCow::Borrowed(v) => v.len(),
            BytesCow::Owned(v) => v.len(),
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl BytesCow<'_> {
    pub fn into_owned(self) -> Box<[u8]> {
        match self {
            BytesCow::Borrowed(v) => v.to_vec().into_boxed_slice(),
            BytesCow::Owned(v) => v,
        }
    }
}

impl<'a> From<BytesCow<'a>> for Vec<u8> {
    fn from(value: BytesCow<'a>) -> Self {
        match value {
            BytesCow::Borrowed(v) => v.to_vec(),
            BytesCow::Owned(v) => v.into_vec(),
        }
    }
}

impl<'a> From<BytesCow<'a>> for Box<[u8]> {
    fn from(value: BytesCow<'a>) -> Self {
        value.into_owned()
    }
}

impl<'a> AsRef<[u8]> for BytesCow<'a> {
    fn as_ref(&self) -> &[u8] {
        match self {
            BytesCow::Borrowed(v) => v,
            BytesCow::Owned(v) => v.as_ref(),
        }
    }
}

#[derive(Default)]
pub struct BytesVisitor<'a>(PhantomData<&'a ()>);

impl<'a> BytesVisitor<'a> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<'a> Visitor<'a> for BytesVisitor<'a> {
    type Value = BytesCow<'a>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("bytes (ByteVisitor in template_lib_types)")
    }

    fn visit_borrowed_bytes<E>(self, v: &'a [u8]) -> Result<Self::Value, E>
    where E: Error {
        Ok(BytesCow::Borrowed(v))
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where E: Error {
        Ok(BytesCow::Owned(v.into_boxed_slice()))
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where E: Error {
        Ok(BytesCow::Owned(v.to_vec().into_boxed_slice()))
    }
}

pub mod fixed_hex {
    use super::*;
    use crate::hex::{bytes_to_hex, fixed_bytes_from_hex};

    pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            let st = bytes_to_hex(v.as_ref());
            s.serialize_str(&st)
        } else {
            s.serialize_bytes(v.as_ref())
        }
    }

    pub fn deserialize<'de, D, T, const L: usize>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: From<[u8; L]>,
    {
        let value = if d.is_human_readable() {
            let hex = String::deserialize(d)?;
            let bytes = fixed_bytes_from_hex(&hex).map_err(Error::custom)?;
            T::from(bytes)
        } else {
            struct FixedBytesVisitor<const L: usize>;

            impl<const L: usize> Visitor<'_> for FixedBytesVisitor<L> {
                type Value = [u8; L];

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("a fixed size byte array")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where E: Error {
                    let mut buf = [0u8; L];
                    if v.len() != L {
                        return Err(E::custom(format!("Expected {} bytes, got {}", L, v.len())));
                    }
                    buf.copy_from_slice(v);
                    Ok(buf)
                }
            }
            let bytes: [u8; L] = d.deserialize_bytes(FixedBytesVisitor::<L>)?;
            T::from(bytes)
        };

        Ok(value)
    }
}

pub mod dynamic_hex {
    use super::*;
    use crate::hex::{bytes_from_hex, bytes_to_hex};

    pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            let st = bytes_to_hex(v.as_ref());
            s.serialize_str(&st)
        } else {
            s.serialize_bytes(v.as_ref())
        }
    }

    pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        for<'a> T: TryFrom<&'a [u8]>,
    {
        if d.is_human_readable() {
            let hex = Box::<str>::deserialize(d)?;
            let bytes = bytes_from_hex(&hex).map_err(Error::custom)?;
            return T::try_from(&bytes)
                .map_err(|_| Error::custom(format!("Failed to convert bytes to type: {}", any::type_name::<T>())));
        }

        let bytes = d.deserialize_byte_buf(BytesVisitor::default())?;
        T::try_from(bytes.as_ref())
            .map_err(|_| Error::custom(format!("Failed to convert bytes to type: {}", any::type_name::<T>())))
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use serde::Serialize;

    use super::*;
    use crate::crypto::RistrettoPublicKeyBytes;

    #[derive(Serialize, Deserialize, Debug)]
    struct TestCase {
        #[serde(with = "super::dynamic_hex")]
        bytes: Vec<u8>,
        pk: RistrettoPublicKeyBytes,
    }

    #[test]
    fn encode_decode_cbor() {
        let test_case = TestCase {
            bytes: vec![1, 2, 3, 4, 5],
            pk: RistrettoPublicKeyBytes::from([1; 32]),
        };
        let encoded = tari_bor::encode(&test_case).unwrap();
        let decoded: TestCase = tari_bor::decode(&encoded).unwrap();

        assert_eq!(test_case.bytes, decoded.bytes);
        assert_eq!(test_case.pk, decoded.pk);

        let cbor = tari_bor::to_value(&test_case).unwrap();
        let decoded = tari_bor::from_value::<TestCase>(&cbor).unwrap();
        assert_eq!(test_case.bytes, decoded.bytes);
        assert_eq!(test_case.pk, decoded.pk);
        // Check encoded as bytes
        let bytes = cbor.as_map().unwrap().first().unwrap().1.as_bytes().unwrap();
        assert_eq!(bytes, &test_case.bytes);
        // Check encoded as public key
        let pk = cbor.as_map().unwrap().get(1).unwrap().1.as_bytes().unwrap();
        assert_eq!(pk, &test_case.pk.as_bytes());
    }

    #[test]
    fn decode_encode_json() {
        let test_case = TestCase {
            bytes: vec![1, 2, 3, 4, 5],
            pk: RistrettoPublicKeyBytes::from([1; 32]),
        };
        let json = serde_json::to_string(&test_case).unwrap();
        let decoded: TestCase = serde_json::from_str(&json).unwrap();

        assert_eq!(test_case.bytes, decoded.bytes);
        assert_eq!(test_case.pk, decoded.pk);

        let json = serde_json::to_value(&test_case).unwrap();
        assert_eq!(json["bytes"].as_str().expect("string"), "0102030405");
        assert_eq!(
            json["pk"].as_str().expect("string"),
            "0101010101010101010101010101010101010101010101010101010101010101"
        );

        let decoded = tari_bor::decode::<TestCase>(&tari_bor::encode(&test_case).unwrap()).unwrap();
        assert_eq!(test_case.bytes, decoded.bytes);
        assert_eq!(test_case.pk, decoded.pk);

        let cbor = tari_bor::to_value(&test_case).unwrap();
        let decoded = tari_bor::from_value::<TestCase>(&cbor).unwrap();
        assert_eq!(test_case.bytes, decoded.bytes);
        assert_eq!(test_case.pk, decoded.pk);
    }
}
