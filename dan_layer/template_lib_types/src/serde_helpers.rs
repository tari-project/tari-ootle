//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, marker::PhantomData};

use ::serde::{
    de::{Error, Visitor},
    Deserialize,
    Deserializer,
    Serializer,
};
use tari_template_abi::rust::borrow::Cow;

use crate::HashParseError;
pub(crate) fn fixed_bytes_from_hex<const L: usize>(s: &str) -> Result<[u8; L], HashParseError> {
    if s.len() != L * 2 {
        return Err(HashParseError);
    }

    let mut bytes = [0u8; L];
    for (i, h) in bytes.iter_mut().enumerate() {
        *h = u8::from_str_radix(&s[2 * i..2 * (i + 1)], 16).map_err(|_| HashParseError)?;
    }
    Ok(bytes)
}

pub(crate) fn bytes_from_hex(s: &str) -> Result<Vec<u8>, HashParseError> {
    if s.len() % 2 != 0 {
        return Err(HashParseError);
    }

    let mut bytes = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| HashParseError)?;
        bytes.push(byte);
    }
    Ok(bytes)
}

pub(crate) fn bytes_to_hex<T: AsRef<[u8]>>(bytes: T) -> String {
    let mut hex = String::with_capacity(bytes.as_ref().len() * 2);
    for byte in bytes.as_ref() {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

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

    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &Option<T>, s: S) -> Result<S::Ok, S::Error> {
            if s.is_human_readable() {
                match v {
                    Some(value) => {
                        let st = bytes_to_hex(value.as_ref());
                        s.serialize_some(&st)
                    },
                    None => s.serialize_none(),
                }
            } else {
                match v {
                    Some(value) => s.serialize_some(value.as_ref()),
                    None => s.serialize_none(),
                }
            }
        }

        pub fn deserialize<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
        where
            D: Deserializer<'de>,
            for<'a> T: TryFrom<&'a [u8]>,
        {
            if d.is_human_readable() {
                let hex = <Option<Cow<'_, str>> as Deserialize>::deserialize(d)?;
                match hex {
                    Some(hex_str) => {
                        let bytes = bytes_from_hex(&hex_str).map_err(Error::custom)?;
                        T::try_from(&bytes).map(Some).map_err(|_| {
                            Error::custom(format!(
                                "Failed to convert bytes to type: {}",
                                std::any::type_name::<T>()
                            ))
                        })
                    },
                    None => Ok(None),
                }
            } else {
                let bytes = <Option<Cow<'_, [u8]>>>::deserialize(d)?;
                match bytes {
                    Some(b) => T::try_from(&b).map(Some).map_err(|_| {
                        Error::custom(format!(
                            "Failed to convert bytes to type: {}",
                            std::any::type_name::<T>()
                        ))
                    }),
                    None => Ok(None),
                }
            }
        }
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
        #[serde(with = "super::dynamic_hex::option")]
        opt_bytes: Option<Vec<u8>>,
    }

    #[test]
    fn decode_encode() {
        let test_case = TestCase {
            bytes: vec![1, 2, 3, 4, 5],
            pk: RistrettoPublicKeyBytes::from([1; 32]),
            opt_bytes: Some(vec![1, 2, 3, 4, 5]),
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
        assert_eq!(json["opt_bytes"].as_str().expect("string"), "0102030405");

        let decoded = tari_bor::decode::<TestCase>(&tari_bor::encode(&test_case).unwrap()).unwrap();
        assert_eq!(test_case.bytes, decoded.bytes);
        assert_eq!(test_case.pk, decoded.pk);

        let cbor = tari_bor::to_value(&test_case).unwrap();
        let decoded = tari_bor::from_value::<TestCase>(&cbor).unwrap();
        assert_eq!(test_case.bytes, decoded.bytes);
        assert_eq!(test_case.pk, decoded.pk);

        let test_case = TestCase {
            bytes: vec![1, 2, 3, 4, 5],
            pk: RistrettoPublicKeyBytes::from([1; 32]),
            opt_bytes: None,
        };
        let json = serde_json::to_value(&test_case).unwrap();
        assert!(json["opt_bytes"].is_null());
    }
}
