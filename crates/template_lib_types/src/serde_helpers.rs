//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ::serde::{
    Deserialize,
    Deserializer,
    Serializer,
    de::{Error, SeqAccess, Visitor},
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

const MAX_PREALLOC_BYTES: usize = 1024;

#[derive(Default)]
pub struct BytesVisitor<'a> {
    max_len: Option<usize>,
    _marker: PhantomData<&'a ()>,
}

impl<'a> BytesVisitor<'a> {
    pub const fn new() -> Self {
        Self {
            max_len: None,
            _marker: PhantomData,
        }
    }

    pub const fn with_max_len(max_len: usize) -> Self {
        Self {
            max_len: Some(max_len),
            _marker: PhantomData,
        }
    }

    fn check_len<E>(&self, len: usize) -> Result<(), E>
    where E: Error {
        if let Some(max) = self.max_len &&
            len > max
        {
            return Err(E::invalid_length(len, self));
        }
        Ok(())
    }
}

impl<'a> Visitor<'a> for BytesVisitor<'a> {
    type Value = BytesCow<'a>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self.max_len {
            Some(max) => write!(formatter, "at most {max} bytes"),
            None => formatter.write_str("bytes (ByteVisitor in template_lib_types)"),
        }
    }

    fn visit_borrowed_bytes<E>(self, v: &'a [u8]) -> Result<Self::Value, E>
    where E: Error {
        self.check_len(v.len())?;
        Ok(BytesCow::Borrowed(v))
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where E: Error {
        self.check_len(v.len())?;
        Ok(BytesCow::Owned(v.into_boxed_slice()))
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where E: Error {
        self.check_len(v.len())?;
        Ok(BytesCow::Owned(v.to_vec().into_boxed_slice()))
    }

    /// A self-describing human-readable format (e.g. JSON) has no native byte type, so `serialize_bytes` is rendered
    /// as a sequence of integers and arrives here.
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where A: SeqAccess<'a> {
        let max_prealloc = self
            .max_len
            .map_or(MAX_PREALLOC_BYTES, |max| max.min(MAX_PREALLOC_BYTES));
        let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(max_prealloc));
        while let Some(byte) = seq.next_element::<u8>()? {
            if let Some(max) = self.max_len &&
                bytes.len() >= max
            {
                return Err(A::Error::invalid_length(bytes.len() + 1, &self));
            }
            bytes.push(byte);
        }
        Ok(BytesCow::Owned(bytes.into_boxed_slice()))
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
    }

    #[test]
    fn bytes_visitor_caps_dishonest_seq_size_hint() {
        use serde::de::{DeserializeSeed, SeqAccess, value::Error as ValueError};

        struct DishonestSeq;
        impl<'de> SeqAccess<'de> for DishonestSeq {
            type Error = ValueError;

            fn next_element_seed<T>(&mut self, _seed: T) -> Result<Option<T::Value>, Self::Error>
            where T: DeserializeSeed<'de> {
                Ok(None)
            }

            fn size_hint(&self) -> Option<usize> {
                Some(usize::MAX)
            }
        }

        let bytes = BytesVisitor::new().visit_seq(DishonestSeq).unwrap();
        assert!(bytes.is_empty());
    }

    #[test]
    fn bytes_visitor_enforces_max_len() {
        use serde::de::value::Error as ValueError;

        let ok = BytesVisitor::with_max_len(4).visit_bytes::<ValueError>(&[1, 2, 3, 4]).unwrap();
        assert_eq!(ok.as_ref(), &[1, 2, 3, 4]);
        assert!(BytesVisitor::with_max_len(4).visit_bytes::<ValueError>(&[1, 2, 3, 4, 5]).is_err());
        assert!(
            BytesVisitor::with_max_len(4)
                .visit_borrowed_bytes::<ValueError>(&[1, 2, 3, 4, 5])
                .is_err()
        );
        assert!(
            BytesVisitor::with_max_len(4)
                .visit_byte_buf::<ValueError>(vec![1, 2, 3, 4, 5])
                .is_err()
        );
    }
}
