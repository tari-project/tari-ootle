//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ::serde::{
    de::{Error, Visitor},
    Deserialize,
    Deserializer,
    Serializer,
};
use tari_template_abi::rust::{borrow::Cow, fmt, marker::PhantomData};

#[derive(Default)]
struct BytesVisitor<'a>(PhantomData<&'a ()>);

impl<'a> Visitor<'a> for BytesVisitor<'a> {
    type Value = Cow<'a, [u8]>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("bytes")
    }

    fn visit_borrowed_bytes<E>(self, v: &'a [u8]) -> Result<Self::Value, E>
    where E: Error {
        Ok(Cow::Borrowed(v))
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where E: Error {
        Ok(Cow::Owned(v))
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where E: Error {
        Ok(Cow::Owned(v.to_vec()))
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
            let hex = <Cow<'_, str> as Deserialize>::deserialize(d)?;
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
            let hex = <Cow<'_, str> as Deserialize>::deserialize(d)?;
            let bytes = bytes_from_hex(&hex).map_err(Error::custom)?;
            return T::try_from(&bytes).map_err(|_| {
                Error::custom(format!(
                    "Failed to convert bytes to type: {}",
                    std::any::type_name::<T>()
                ))
            });
        }

        let bytes: Cow<'_, [u8]> = d.deserialize_bytes(BytesVisitor::default())?;
        T::try_from(&bytes).map_err(|_| {
            Error::custom(format!(
                "Failed to convert bytes to type: {}",
                std::any::type_name::<T>()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
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
