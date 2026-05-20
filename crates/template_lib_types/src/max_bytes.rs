//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::{
    format,
    ops::{Deref, DerefMut},
    prelude::*,
};

#[cfg(feature = "serde")]
use crate::{
    hex::{bytes_from_hex, bytes_to_hex},
    serde_helpers::BytesVisitor,
};

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct MaxBytes<const N: usize>(Box<[u8]>);

impl<const N: usize> Deref for MaxBytes<N> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const N: usize> MaxBytes<N> {
    pub fn new() -> Self {
        Self::empty()
    }

    pub fn empty() -> Self {
        Self(Box::new([]))
    }

    /// Constructs a new `MaxBytes<N>` if the length of the input is less than or equal to `N`.
    /// Returns `None` if the length exceeds `N`.
    pub fn new_checked(bytes: impl Into<Box<[u8]>>) -> Option<Self> {
        let bytes = bytes.into();
        if bytes.len() <= N { Some(Self(bytes)) } else { None }
    }

    /// Constructs a new `MaxBytes<N>` without checking the length of the input.
    /// This is the only way to break the invariant guarantees of `MaxBytes<N>`.
    /// NOTE: this exists for testing purposes and should not be used in general.
    ///
    /// # Safety
    /// The caller must ensure that the length of `bytes` is less than or equal to `N`.
    pub unsafe fn new_unchecked(bytes: impl Into<Box<[u8]>>) -> Self {
        Self(bytes.into())
    }

    pub fn into_bytes(self) -> Box<[u8]> {
        self.0
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.into_bytes().into_vec()
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl<const N: usize> AsRef<[u8]> for MaxBytes<N> {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl<const N: usize> DerefMut for MaxBytes<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Mutable but not resizeable
        &mut self.0
    }
}

impl<const N: usize> Default for MaxBytes<N> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<const N: usize> TryFrom<Vec<u8>> for MaxBytes<N> {
    type Error = ();

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::new_checked(value).ok_or(())
    }
}

impl<const N: usize> TryFrom<Box<[u8]>> for MaxBytes<N> {
    type Error = ();

    fn try_from(value: Box<[u8]>) -> Result<Self, Self::Error> {
        Self::new_checked(value).ok_or(())
    }
}

impl<const N: usize> From<MaxBytes<N>> for Vec<u8> {
    fn from(value: MaxBytes<N>) -> Self {
        value.into_vec()
    }
}

#[cfg(feature = "serde")]
impl<const N: usize> serde::Serialize for MaxBytes<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        if serializer.is_human_readable() {
            let hex = bytes_to_hex(self.as_slice());
            serializer.serialize_str(&hex)
        } else {
            serializer.serialize_bytes(self.as_slice())
        }
    }
}

#[cfg(feature = "serde")]
impl<'de, const N: usize> serde::Deserialize<'de> for MaxBytes<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de> {
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            if s.len() > N * 2 {
                return Err(serde::de::Error::custom(format!(
                    "byte array length exceeds maximum of {}",
                    N
                )));
            }
            let bytes = bytes_from_hex(&s).map_err(serde::de::Error::custom)?;
            MaxBytes::new_checked(bytes.into_boxed_slice())
                .ok_or_else(|| serde::de::Error::custom(format!("byte array length exceeds maximum of {}", N)))
        } else {
            let bytes = deserializer.deserialize_byte_buf(BytesVisitor::new())?;
            if bytes.len() > N {
                return Err(serde::de::Error::custom(format!(
                    "byte array length exceeds maximum of {}",
                    N
                )));
            }
            Ok(MaxBytes::new_checked(bytes.into_owned()).expect("length checked above"))
        }
    }
}

impl<C, const N: usize> minicbor::Encode<C> for MaxBytes<N> {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.bytes(self.as_slice())?;
        Ok(())
    }
}

impl<'b, C, const N: usize> minicbor::Decode<'b, C> for MaxBytes<N> {
    fn decode(d: &mut minicbor::Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        if bytes.len() > N {
            return Err(minicbor::decode::Error::message(format!(
                "byte array length exceeds maximum of {}",
                N
            )));
        }
        Ok(MaxBytes::new_checked(bytes.to_vec().into_boxed_slice()).expect("length checked above"))
    }
}

impl<C, const N: usize> minicbor::CborLen<C> for MaxBytes<N> {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        minicbor::bytes::cbor_len(self.as_slice(), ctx)
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use super::*;

    mod new_checked {
        use super::*;

        #[test]
        fn it_returns_some_if_data_le_size() {
            let b = vec![1, 2, 3];
            let mb = MaxBytes::<5>::new_checked(b).unwrap();
            assert_eq!(mb.len(), 3);
            assert_eq!(&mb[..], &[1, 2, 3]);
        }

        #[test]
        fn it_returns_none_if_data_gt_size() {
            let b = vec![1; 6];
            let mb = MaxBytes::<5>::new_checked(b);
            assert!(mb.is_none());
        }
    }

    mod serde_impl {
        use super::*;

        #[test]
        fn it_deserializes_and_serializes_human_readable() {
            let original = MaxBytes::<5>::new_checked(vec![1, 2, 3, 4, 5]).unwrap();
            let serialized = serde_json::to_string(&original).unwrap();
            assert_eq!(serialized, "\"0102030405\"");
            let deserialized: MaxBytes<5> = serde_json::from_str(&serialized).unwrap();
            assert_eq!(original, deserialized);
        }

        #[test]
        fn it_serializes_and_deserializes_as_bytes() {
            let original = MaxBytes::<5>::new_checked(vec![1, 2, 3, 4, 5]).unwrap();
            let serialized = tari_bor::encode(&original).unwrap();
            // Assert that it encodes to a BOR bytes value using the Bytes variant
            let val: tari_bor::Value = tari_bor::decode(&serialized).unwrap();
            assert_eq!(val, tari_bor::Value::Bytes(vec![1, 2, 3, 4, 5]));
            // Now decode it back
            let deserialized: MaxBytes<5> = tari_bor::decode(&serialized).unwrap();
            assert_eq!(original, deserialized);
        }

        #[test]
        fn it_fails_to_deserialize_if_length_is_too_large() {
            let json = "\"010203040506\""; // 6 bytes, max is 5
            let err: serde_json::Error = serde_json::from_str::<MaxBytes<5>>(json).unwrap_err();
            assert!(err.to_string().contains("byte array length exceeds maximum"));

            let bytes = MaxBytes::<5>(vec![1; 6].into_boxed_slice());
            let serialized = tari_bor::encode(&bytes).unwrap();
            let err = tari_bor::decode::<MaxBytes<5>>(&serialized).unwrap_err();
            assert!(err.to_string().contains("byte array length exceeds maximum"));
        }
    }
}
