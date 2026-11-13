// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::borrow::Cow;

use serde::{Deserialize, Deserializer, Serializer};
/// Encodes a byte type as a hex string if the serializer is human-readable, Otherwise efficiently encodes it as
/// bytes.
/// This is different from the standard serde byte encoding, which encodes a sequence of u8s (e.g. [1, 2, 3]) as a JSON
/// array.
pub mod hex_or_bytes {
    use super::*;

    pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            let st = ::hex::encode(v.as_ref());
            s.serialize_str(&st)
        } else {
            s.serialize_bytes(v.as_ref())
        }
    }

    /// Use a serde deserializer to serialize the hex string of the given object.
    pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: for<'a> TryFrom<&'a [u8]>,
    {
        let value = if d.is_human_readable() {
            let hex = <Cow<'_, str> as Deserialize>::deserialize(d)?;
            let bytes = ::hex::decode(&*hex).map_err(serde::de::Error::custom)?;
            T::try_from(&bytes).map_err(|_| serde::de::Error::custom("Failed to convert bytes to T"))?
        } else {
            let bytes = <Cow<'_, [u8]> as Deserialize>::deserialize(d)?;
            T::try_from(&bytes).map_err(|_| serde::de::Error::custom("Failed to convert bytes to T"))?
        };

        Ok(value)
    }
}

pub mod hex {
    use tari_utilities::hex::{from_hex, to_hex};

    use super::*;

    pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.serialize_str(&to_hex(v.as_ref()))
        } else {
            s.serialize_bytes(v.as_ref())
        }
    }

    pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: TryFrom<Vec<u8>>,
    {
        let bytes = if d.is_human_readable() {
            let hex = <String as Deserialize>::deserialize(d)?;
            from_hex(&hex).map_err(serde::de::Error::custom)?
        } else {
            <Cow<'_, [u8]> as Deserialize>::deserialize(d)?.into_owned()
        };

        T::try_from(bytes).map_err(|_| serde::de::Error::custom("Failed to convert bytes to T"))
    }
}

pub mod hex_seq {
    use serde::Serialize;

    use super::*;

    pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &[T], s: S) -> Result<S::Ok, S::Error> {
        struct Item<'a>(&'a [u8]);

        impl Serialize for Item<'_> {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                super::hex::serialize(&self.0, s)
            }
        }

        s.collect_seq(v.iter().map(|item| Item(item.as_ref())))
    }

    pub fn deserialize<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: TryFrom<Vec<u8>>,
    {
        struct Item<T>(T);

        impl<'de, T: TryFrom<Vec<u8>>> Deserialize<'de> for Item<T> {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                super::hex::deserialize(d).map(Item)
            }
        }

        let items = Vec::<Item<T>>::deserialize(d)?;
        Ok(items.into_iter().map(|item| item.0).collect())
    }
}

pub mod base64 {
    use ::base64::{Engine as _, engine::general_purpose::STANDARD};

    use super::*;

    pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.serialize_str(&STANDARD.encode(v))
        } else {
            s.serialize_bytes(v.as_ref())
        }
    }

    pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: TryFrom<Vec<u8>>,
    {
        let bytes = if d.is_human_readable() {
            let s = String::deserialize(d)?;
            STANDARD.decode(s.as_bytes()).map_err(serde::de::Error::custom)?
        } else {
            <Cow<'_, [u8]> as Deserialize>::deserialize(d)?.into_owned()
        };

        T::try_from(bytes).map_err(|_| serde::de::Error::custom("Failed to convert bytes to T"))
    }
}

#[cfg(test)]
mod tests {
    use serde::{
        de::{SeqAccess, Visitor, value::Error as ValueError},
        forward_to_deserialize_any,
    };

    use super::*;
    use crate::FixedHash;

    struct BinaryBytesSerializer;

    impl Serializer for BinaryBytesSerializer {
        type Error = ValueError;
        type Ok = Vec<u8>;
        type SerializeMap = serde::ser::Impossible<Vec<u8>, ValueError>;
        type SerializeSeq = serde::ser::Impossible<Vec<u8>, ValueError>;
        type SerializeStruct = serde::ser::Impossible<Vec<u8>, ValueError>;
        type SerializeStructVariant = serde::ser::Impossible<Vec<u8>, ValueError>;
        type SerializeTuple = serde::ser::Impossible<Vec<u8>, ValueError>;
        type SerializeTupleStruct = serde::ser::Impossible<Vec<u8>, ValueError>;
        type SerializeTupleVariant = serde::ser::Impossible<Vec<u8>, ValueError>;

        fn is_human_readable(&self) -> bool {
            false
        }

        fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
            Ok(v.to_vec())
        }

        fn serialize_bool(self, _: bool) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_i8(self, _: i8) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_i16(self, _: i16) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_i32(self, _: i32) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_i64(self, _: i64) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_u8(self, _: u8) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_u16(self, _: u16) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_u32(self, _: u32) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_u64(self, _: u64) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_f32(self, _: f32) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_f64(self, _: f64) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_char(self, _: char) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_str(self, _: &str) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_some<T: ?Sized + serde::Serialize>(self, _: &T) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_newtype_struct<T: ?Sized + serde::Serialize>(
            self,
            _: &'static str,
            _: &T,
        ) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_newtype_variant<T: ?Sized + serde::Serialize>(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: &T,
        ) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_tuple_struct(
            self,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeTupleStruct, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_tuple_variant(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeTupleVariant, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }

        fn serialize_struct_variant(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeStructVariant, Self::Error> {
            Err(serde::ser::Error::custom("unexpected"))
        }
    }

    struct BinaryBytesDeserializer<'de>(&'de [u8]);

    impl<'de> Deserializer<'de> for BinaryBytesDeserializer<'de> {
        type Error = ValueError;

        fn is_human_readable(&self) -> bool {
            false
        }

        fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            visitor.visit_borrowed_bytes(self.0)
        }

        forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map struct enum identifier ignored_any
        }
    }

    struct BinarySeqOfBytesDeserializer<'de>(&'de [&'de [u8]]);

    impl<'de> Deserializer<'de> for BinarySeqOfBytesDeserializer<'de> {
        type Error = ValueError;

        fn is_human_readable(&self) -> bool {
            false
        }

        fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            struct SliceSeq<'a, 'de>(&'a [&'de [u8]]);

            impl<'de> SeqAccess<'de> for SliceSeq<'_, 'de> {
                type Error = ValueError;

                fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
                where T: serde::de::DeserializeSeed<'de> {
                    let Some((first, rest)) = self.0.split_first() else {
                        return Ok(None);
                    };
                    self.0 = rest;
                    seed.deserialize(BinaryBytesDeserializer(first)).map(Some)
                }
            }

            visitor.visit_seq(SliceSeq(self.0))
        }

        forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map struct enum identifier ignored_any
        }
    }

    #[test]
    fn hex_and_base64_round_trip_with_non_human_readable_bytes() {
        let hash = FixedHash::from([0xab; 32]);
        let encoded_hex = hex::serialize(&hash, BinaryBytesSerializer).unwrap();
        assert_eq!(encoded_hex, [0xab; 32]);
        let decoded_hex: FixedHash = hex::deserialize(BinaryBytesDeserializer(&encoded_hex)).unwrap();
        assert_eq!(decoded_hex, hash);

        let payload = vec![1u8, 2, 3, 4, 5];
        let encoded_b64 = base64::serialize(&payload, BinaryBytesSerializer).unwrap();
        assert_eq!(encoded_b64, payload);
        let decoded_b64: Vec<u8> = base64::deserialize(BinaryBytesDeserializer(&encoded_b64)).unwrap();
        assert_eq!(decoded_b64, payload);

        let item_a = [0x11u8; 32];
        let item_b = [0x22u8; 32];
        let slices: &[&[u8]] = &[&item_a, &item_b];
        let decoded_seq: Vec<FixedHash> = hex_seq::deserialize(BinarySeqOfBytesDeserializer(slices)).unwrap();
        assert_eq!(decoded_seq, vec![FixedHash::from(item_a), FixedHash::from(item_b)]);
    }
}
