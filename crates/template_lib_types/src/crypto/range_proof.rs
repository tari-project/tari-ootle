//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::{fmt, prelude::*};

/// A range proof is a cryptographic proof that a value is within a certain range without revealing the value itself.
/// This struct is used to represent the range proof bytes in a serialized format.
/// The length of the range proof is limited to a maximum size (1024 bytes) to mitigate potential denial-of-service
/// attacks.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct RangeProofBytes(
    #[cfg_attr(
        feature = "serde",
        serde(
            serialize_with = "crate::serde_helpers::dynamic_hex::serialize",
            deserialize_with = "serde_validated_range_proof_deserialize_impl"
        )
    )]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    Vec<u8>,
);

impl<C> minicbor::Encode<C> for RangeProofBytes {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.bytes(&self.0)?;
        Ok(())
    }
}

impl<'b, C> minicbor::Decode<'b, C> for RangeProofBytes {
    fn decode(d: &mut minicbor::Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        if bytes.len() > Self::MAX_LENGTH {
            return Err(minicbor::decode::Error::message("range proof exceeds maximum length"));
        }
        Ok(Self(bytes.to_vec()))
    }
}

impl<C> minicbor::CborLen<C> for RangeProofBytes {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        minicbor::bytes::cbor_len(self.0.as_slice(), ctx)
    }
}

impl RangeProofBytes {
    // TODO: is this sufficiently large? How many outputs can we aggregate before we hit this limit?
    // observed size for 2 commitments is ~500 bytes. For 500 commitments, it is 1153 bytes.
    pub const MAX_LENGTH: usize = 1153;

    pub const fn empty() -> Self {
        Self(Vec::new())
    }

    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub const fn len(&self) -> usize {
        self.0.len()
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl AsRef<Vec<u8>> for RangeProofBytes {
    fn as_ref(&self) -> &Vec<u8> {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for RangeProofBytes {
    type Error = RangeProofSizeExceeded;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        if bytes.len() > Self::MAX_LENGTH {
            return Err(RangeProofSizeExceeded { size: bytes.len() });
        }
        Ok(Self(bytes))
    }
}

/// Ensures deserialized bytes are a valid size
#[cfg(feature = "serde")]
fn serde_validated_range_proof_deserialize_impl<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where D: serde::Deserializer<'de> {
    if deserializer.is_human_readable() {
        struct RangeProofHexVisitor;

        impl serde::de::Visitor<'_> for RangeProofHexVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(
                    formatter,
                    "a hex string of at most {} bytes",
                    RangeProofBytes::MAX_LENGTH
                )
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where E: serde::de::Error {
                if v.len() > RangeProofBytes::MAX_LENGTH * 2 {
                    return Err(E::custom(RangeProofSizeExceeded {
                        size: v.len().div_ceil(2),
                    }));
                }
                crate::hex::bytes_from_hex(v).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(RangeProofHexVisitor)
    } else {
        let bytes = deserializer.deserialize_byte_buf(crate::serde_helpers::BytesVisitor::with_max_len(
            RangeProofBytes::MAX_LENGTH,
        ))?;
        Ok(bytes.into())
    }
}

#[derive(Debug)]
pub struct RangeProofSizeExceeded {
    size: usize,
}

impl fmt::Display for RangeProofSizeExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Range proof size ({} bytes) exceeded the maximum ({} bytes).",
            self.size,
            RangeProofBytes::MAX_LENGTH
        )
    }
}

#[cfg(feature = "std")]
impl std::error::Error for RangeProofSizeExceeded {}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use serde::Deserialize;

    use super::*;

    #[test]
    fn cbor_oversized_range_proof_array_stops_before_reading_excess_elements() {
        use serde::de::{DeserializeSeed, Deserializer, SeqAccess, Visitor, value::Error as ValueError};

        struct CountingSeq {
            remaining: usize,
            reads: Rc<Cell<usize>>,
        }

        impl<'de> SeqAccess<'de> for CountingSeq {
            type Error = ValueError;

            fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
            where T: DeserializeSeed<'de> {
                if self.remaining == 0 {
                    return Ok(None);
                }
                self.remaining -= 1;
                self.reads.set(self.reads.get() + 1);
                seed.deserialize(serde::de::IntoDeserializer::<ValueError>::into_deserializer(0u8))
                    .map(Some)
            }

            fn size_hint(&self) -> Option<usize> {
                Some(self.remaining)
            }
        }

        struct BinarySeqDe {
            len: usize,
            reads: Rc<Cell<usize>>,
        }

        impl<'de> Deserializer<'de> for BinarySeqDe {
            type Error = ValueError;

            fn is_human_readable(&self) -> bool {
                false
            }

            fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
            where V: Visitor<'de> {
                visitor.visit_seq(CountingSeq {
                    remaining: self.len,
                    reads: self.reads,
                })
            }

            fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, Self::Error>
            where V: Visitor<'de> {
                visitor.visit_newtype_struct(self)
            }

            fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
            where V: Visitor<'de> {
                self.deserialize_byte_buf(visitor)
            }

            serde::forward_to_deserialize_any! {
                bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
                bytes option unit unit_struct seq tuple
                tuple_struct map struct enum identifier ignored_any
            }
        }

        let reads = Rc::new(Cell::new(0));
        let res = RangeProofBytes::deserialize(BinarySeqDe {
            len: RangeProofBytes::MAX_LENGTH + 500,
            reads: reads.clone(),
        });
        assert!(res.is_err());
        assert!(
            reads.get() <= RangeProofBytes::MAX_LENGTH + 1,
            "expected deserializer to stop once MAX_LENGTH is exceeded, read {} elements",
            reads.get()
        );

        let exact_reads = Rc::new(Cell::new(0));
        let exact = RangeProofBytes::deserialize(BinarySeqDe {
            len: RangeProofBytes::MAX_LENGTH,
            reads: exact_reads.clone(),
        })
        .unwrap();
        assert_eq!(exact.len(), RangeProofBytes::MAX_LENGTH);
        assert_eq!(exact_reads.get(), RangeProofBytes::MAX_LENGTH);
    }

    #[test]
    fn serde_json_and_cbor_enforce_max_length() {
        let valid = RangeProofBytes::try_from(vec![0xabu8; RangeProofBytes::MAX_LENGTH]).unwrap();
        let json = serde_json::to_string(&valid).unwrap();
        let decoded_json: RangeProofBytes = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded_json, valid);

        let cbor = tari_bor::serde_codec::to_vec(&valid).unwrap();
        let decoded_cbor: RangeProofBytes = tari_bor::serde_codec::from_slice(&cbor).unwrap();
        assert_eq!(decoded_cbor, valid);

        let oversized_hex = format!("\"{}\"", "ab".repeat(RangeProofBytes::MAX_LENGTH + 1));
        assert!(serde_json::from_str::<RangeProofBytes>(&oversized_hex).is_err());

        let odd_oversized_hex = format!("\"{}a\"", "ab".repeat(RangeProofBytes::MAX_LENGTH));
        assert!(serde_json::from_str::<RangeProofBytes>(&odd_oversized_hex).is_err());

        let oversized_raw = vec![0xabu8; RangeProofBytes::MAX_LENGTH + 1];
        let oversized_cbor = tari_bor::encode(&crate::bytes::Bytes::from_vec(oversized_raw)).unwrap();
        assert!(tari_bor::serde_codec::from_slice::<RangeProofBytes>(&oversized_cbor).is_err());
    }
}
