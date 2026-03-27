//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use serde::{
    Serialize,
    Serializer,
    de::{self, Visitor},
};

pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
    v.serialize(s)
}

/// Deserializes a `u64` from either a JSON number or a JSON string.
///
/// This handles the case where a JavaScript client serializes large `u64` values as strings
/// because they exceed the client's safe integer threshold.
pub fn deserialize<'de, D>(d: D) -> Result<u64, D::Error>
where D: de::Deserializer<'de> {
    struct U64OrStr;

    impl<'de> Visitor<'de> for U64OrStr {
        type Value = u64;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a u64 integer or a string containing a u64 integer")
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<u64, E> {
            Ok(v)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<u64, E> {
            u64::try_from(v).map_err(|_| E::custom(format!("invalid value: {v}")))
        }

        fn visit_u128<E: de::Error>(self, v: u128) -> Result<u64, E> {
            u64::try_from(v).map_err(|_| E::custom(format!("value too large for u64: {v}")))
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<u64, E> {
            v.parse().map_err(E::custom)
        }

        fn visit_string<E: de::Error>(self, v: String) -> Result<u64, E> {
            self.visit_str(&v)
        }
    }

    if d.is_human_readable() {
        d.deserialize_any(U64OrStr)
    } else {
        d.deserialize_u64(U64OrStr)
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

    // --- Bincode (binary, non-human-readable) ---

    #[test]
    fn round_trip_bincode() {
        for value in [0, 1, u64::from(u32::MAX), u64::MAX] {
            let encoded = bincode::serde::encode_to_vec(w(value), bincode::config::standard()).unwrap();
            let (decoded, _): (Wrapper, _) =
                bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
            assert_eq!(decoded, w(value));
        }
    }
}
