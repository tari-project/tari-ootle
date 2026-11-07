//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! (De)serialization for 64-bit integers that may arrive as a JSON number or a JSON string.
//!
//! JavaScript cannot hold an integer above `Number.MAX_SAFE_INTEGER` (2^53 - 1) in a `number`
//! without losing precision, so JS clients encode 64-bit fields as strings. A field using this
//! module accepts either form on the wire.
//!
//! Serialization is unchanged — always a number — so existing consumers are unaffected.

use alloc::{format, string::String};
use core::{any, fmt, marker::PhantomData};

use serde::{
    Serialize,
    Serializer,
    de::{self, Visitor},
};

/// An integer that a JSON client may encode as a string.
///
/// Implemented for the widths that ts-rs maps to a TypeScript `bigint`.
pub trait StrNumber: Sized {
    /// Requests this type's natural integer width from the deserializer.
    ///
    /// Only used for non-self-describing formats, where `deserialize_any` is unavailable.
    fn deserialize_as<'de, D, V>(deserializer: D, visitor: V) -> Result<V::Value, D::Error>
    where
        D: de::Deserializer<'de>,
        V: Visitor<'de>;

    fn from_u64(v: u64) -> Option<Self>;
    fn from_i64(v: i64) -> Option<Self>;
    fn from_u128(v: u128) -> Option<Self>;
    fn from_i128(v: i128) -> Option<Self>;
    fn parse(s: &str) -> Option<Self>;
}

macro_rules! impl_str_number {
    ($ty:ty, $deserialize_as:ident) => {
        impl StrNumber for $ty {
            fn deserialize_as<'de, D, V>(deserializer: D, visitor: V) -> Result<V::Value, D::Error>
            where
                D: de::Deserializer<'de>,
                V: Visitor<'de>,
            {
                deserializer.$deserialize_as(visitor)
            }

            fn from_u64(v: u64) -> Option<Self> {
                Self::try_from(v).ok()
            }

            fn from_i64(v: i64) -> Option<Self> {
                Self::try_from(v).ok()
            }

            fn from_u128(v: u128) -> Option<Self> {
                Self::try_from(v).ok()
            }

            fn from_i128(v: i128) -> Option<Self> {
                Self::try_from(v).ok()
            }

            fn parse(s: &str) -> Option<Self> {
                s.parse().ok()
            }
        }
    };
}

impl_str_number!(u64, deserialize_u64);
impl_str_number!(i64, deserialize_i64);
impl_str_number!(u128, deserialize_u128);
impl_str_number!(i128, deserialize_i128);
impl_str_number!(usize, deserialize_u64);
impl_str_number!(isize, deserialize_i64);

struct StrNumberVisitor<T>(PhantomData<T>);

impl<'de, T: StrNumber> Visitor<'de> for StrNumberVisitor<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "a {} integer or a string containing one", any::type_name::<T>())
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<T, E> {
        T::from_u64(v).ok_or_else(|| E::custom(format!("invalid value: {v}")))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<T, E> {
        T::from_i64(v).ok_or_else(|| E::custom(format!("invalid value: {v}")))
    }

    fn visit_u128<E: de::Error>(self, v: u128) -> Result<T, E> {
        T::from_u128(v).ok_or_else(|| E::custom(format!("invalid value: {v}")))
    }

    fn visit_i128<E: de::Error>(self, v: i128) -> Result<T, E> {
        T::from_i128(v).ok_or_else(|| E::custom(format!("invalid value: {v}")))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
        T::parse(v).ok_or_else(|| E::custom(format!("invalid value: \"{v}\"")))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<T, E> {
        self.visit_str(&v)
    }
}

pub fn serialize<T: Serialize, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error> {
    v.serialize(s)
}

/// Deserializes an integer from either a JSON number or a JSON string.
pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: de::Deserializer<'de>,
    T: StrNumber,
{
    if d.is_human_readable() {
        d.deserialize_any(StrNumberVisitor(PhantomData))
    } else {
        T::deserialize_as(d, StrNumberVisitor(PhantomData))
    }
}

/// The [`mod@self`] encoding for `Option<T>`.
///
/// Pair with `#[serde(default)]` so that an absent field is `None` rather than an error.
pub mod option {
    use super::*;

    struct OptionVisitor<T>(PhantomData<T>);

    impl<'de, T: StrNumber> Visitor<'de> for OptionVisitor<T> {
        type Value = Option<T>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(
                f,
                "null, a {} integer, or a string containing one",
                any::type_name::<T>()
            )
        }

        fn visit_none<E: de::Error>(self) -> Result<Option<T>, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Option<T>, E> {
            Ok(None)
        }

        fn visit_some<D: de::Deserializer<'de>>(self, d: D) -> Result<Option<T>, D::Error> {
            super::deserialize(d).map(Some)
        }
    }

    pub fn serialize<T: Serialize, S: Serializer>(v: &Option<T>, s: S) -> Result<S::Ok, S::Error> {
        v.serialize(s)
    }

    pub fn deserialize<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
    where
        D: de::Deserializer<'de>,
        T: StrNumber,
    {
        d.deserialize_option(OptionVisitor(PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Wrapper {
        #[serde(with = "crate::str_number")]
        value: u64,
    }

    fn w(value: u64) -> Wrapper {
        Wrapper { value }
    }

    // --- JSON (human-readable) ---

    #[test]
    fn deserialize_json_number() {
        let result: Wrapper = serde_json::from_str(r#"{"value":42}"#).unwrap();
        assert_eq!(result, w(42));
    }

    #[test]
    fn deserialize_json_number_zero() {
        let result: Wrapper = serde_json::from_str(r#"{"value":0}"#).unwrap();
        assert_eq!(result, w(0));
    }

    #[test]
    fn deserialize_json_number_above_u32_max() {
        // JS sends numbers above u32::MAX as strings, but plain numbers should still work
        let result: Wrapper = serde_json::from_str(r#"{"value":5000000000}"#).unwrap();
        assert_eq!(result, w(5_000_000_000));
    }

    #[test]
    fn deserialize_json_number_u64_max() {
        let result: Wrapper = serde_json::from_str(r#"{"value":18446744073709551615}"#).unwrap();
        assert_eq!(result, w(u64::MAX));
    }

    #[test]
    fn deserialize_json_string_small() {
        let result: Wrapper = serde_json::from_str(r#"{"value":"42"}"#).unwrap();
        assert_eq!(result, w(42));
    }

    #[test]
    fn deserialize_json_string_zero() {
        let result: Wrapper = serde_json::from_str(r#"{"value":"0"}"#).unwrap();
        assert_eq!(result, w(0));
    }

    #[test]
    fn deserialize_json_string_u64_max() {
        let result: Wrapper = serde_json::from_str(r#"{"value":"18446744073709551615"}"#).unwrap();
        assert_eq!(result, w(u64::MAX));
    }

    #[test]
    fn deserialize_json_negative_number_fails() {
        let err = serde_json::from_str::<Wrapper>(r#"{"value":-1}"#).unwrap_err();
        assert!(err.to_string().contains("invalid value"), "unexpected error: {err}");
    }

    #[test]
    fn deserialize_json_string_negative_fails() {
        serde_json::from_str::<Wrapper>(r#"{"value":"-1"}"#).unwrap_err();
    }

    #[test]
    fn deserialize_json_string_overflow_fails() {
        // One more than u64::MAX
        serde_json::from_str::<Wrapper>(r#"{"value":"18446744073709551616"}"#).unwrap_err();
    }

    #[test]
    fn deserialize_json_invalid_string_fails() {
        serde_json::from_str::<Wrapper>(r#"{"value":"not_a_number"}"#).unwrap_err();
    }

    #[test]
    fn serialize_json_produces_number_not_string() {
        let json = serde_json::to_string(&w(5_000_000_000)).unwrap();
        assert_eq!(json, r#"{"value":5000000000}"#);
    }

    // --- Binary (non-human-readable, tari_bor::serde_codec) ---

    #[test]
    fn round_trip_binary() {
        for value in [0, 1, u64::from(u32::MAX), u64::MAX] {
            let encoded = tari_bor::serde_codec::to_vec(w(value)).unwrap();
            let decoded: Wrapper = tari_bor::serde_codec::from_slice(&encoded).unwrap();
            assert_eq!(decoded, w(value));
        }
    }

    // --- i64 ---

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct SignedWrapper {
        #[serde(with = "crate::str_number")]
        value: i64,
    }

    #[test]
    fn deserialize_i64_from_number_and_string() {
        for (json, expected) in [
            (r#"{"value":-1}"#, -1),
            (r#"{"value":"-1"}"#, -1),
            (r#"{"value":"-9223372036854775808"}"#, i64::MIN),
            (r#"{"value":"9223372036854775807"}"#, i64::MAX),
            (r#"{"value":9223372036854775807}"#, i64::MAX),
        ] {
            let result: SignedWrapper = serde_json::from_str(json).unwrap();
            assert_eq!(result, SignedWrapper { value: expected }, "failed for {json}");
        }
    }

    #[test]
    fn deserialize_i64_overflow_fails() {
        serde_json::from_str::<SignedWrapper>(r#"{"value":"9223372036854775808"}"#).unwrap_err();
        serde_json::from_str::<SignedWrapper>(r#"{"value":18446744073709551615}"#).unwrap_err();
    }

    #[test]
    fn round_trip_binary_i64() {
        for value in [0, -1, i64::MIN, i64::MAX] {
            let encoded = tari_bor::serde_codec::to_vec(SignedWrapper { value }).unwrap();
            let decoded: SignedWrapper = tari_bor::serde_codec::from_slice(&encoded).unwrap();
            assert_eq!(decoded, SignedWrapper { value });
        }
    }

    // --- Option ---

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct OptWrapper {
        #[serde(default, with = "crate::str_number::option")]
        value: Option<u64>,
    }

    fn o(value: Option<u64>) -> OptWrapper {
        OptWrapper { value }
    }

    #[test]
    fn deserialize_option_from_number_string_null_and_absent() {
        for (json, expected) in [
            (r#"{"value":42}"#, Some(42)),
            (r#"{"value":"42"}"#, Some(42)),
            (r#"{"value":"18446744073709551615"}"#, Some(u64::MAX)),
            (r#"{"value":null}"#, None),
            (r#"{}"#, None),
        ] {
            let result: OptWrapper = serde_json::from_str(json).unwrap();
            assert_eq!(result, o(expected), "failed for {json}");
        }
    }

    #[test]
    fn deserialize_option_invalid_string_fails() {
        serde_json::from_str::<OptWrapper>(r#"{"value":"not_a_number"}"#).unwrap_err();
    }

    #[test]
    fn serialize_option_produces_number_or_null() {
        assert_eq!(serde_json::to_string(&o(Some(42))).unwrap(), r#"{"value":42}"#);
        assert_eq!(serde_json::to_string(&o(None)).unwrap(), r#"{"value":null}"#);
    }

    // --- Query strings (axum's `Query` extractor deserializes with serde_urlencoded) ---

    #[test]
    fn deserialize_from_query_string() {
        let result: Wrapper = serde_urlencoded::from_str("value=18446744073709551615").unwrap();
        assert_eq!(result, w(u64::MAX));

        let result: OptWrapper = serde_urlencoded::from_str("value=42").unwrap();
        assert_eq!(result, o(Some(42)));

        let result: OptWrapper = serde_urlencoded::from_str("").unwrap();
        assert_eq!(result, o(None));
    }

    #[test]
    fn round_trip_binary_option() {
        for value in [None, Some(0), Some(u64::MAX)] {
            let encoded = tari_bor::serde_codec::to_vec(o(value)).unwrap();
            let decoded: OptWrapper = tari_bor::serde_codec::from_slice(&encoded).unwrap();
            assert_eq!(decoded, o(value));
        }
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct WideWrapper {
        #[serde(with = "crate::str_number")]
        unsigned: u128,
        #[serde(with = "crate::str_number")]
        signed: i128,
    }

    #[test]
    fn deserialize_u128_and_i128_from_string_and_number() {
        let json = r#"{"unsigned":"340282366920938463463374607431768211455","signed":"-170141183460469231731687303715884105728"}"#;
        let parsed: WideWrapper = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, WideWrapper {
            unsigned: u128::MAX,
            signed: i128::MIN,
        });
    }
}
