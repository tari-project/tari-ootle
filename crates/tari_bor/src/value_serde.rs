//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
//
//! Serde `Serialize` / `Deserialize` for [`Value`].
//!
//! Only used in JSON-style contexts. CBOR encoding always goes through minicbor's
//! `Encode`/`Decode` and never through serde.
//!
//! ## Representation
//!
//! Values that map cleanly onto JSON are serialised directly:
//!
//! | Variant | JSON |
//! |---------|------|
//! | `Null` | `null` |
//! | `Bool(b)` | bool |
//! | `Integer(i)` (fits in i64) | number |
//! | `Integer(i)` (fits in u64 but not i64) | number |
//! | `Float(f)` | number |
//! | `Text(s)` | string |
//! | `Array(a)` | array |
//! | `Map(m)` where every key is `Text` | object |
//!
//! Variants without a natural JSON shape are emitted using a sentinel object keyed on
//! `"@cbor"`:
//!
//! | Variant | JSON |
//! |---------|------|
//! | `Bytes(b)` | `{ "@cbor": "bytes", "hex": "ab12.." }` |
//! | `Integer(i)` (outside i64/u64 range) | `{ "@cbor": "int", "value": "12345" }` |
//! | `Map(m)` with non-text keys | `{ "@cbor": "map", "entries": [[k, v], ...] }` |
//! | `Tag(t, v)` | `{ "@cbor": "tag", "tag": N, "value": v }` |
//!
//! Deserialisation accepts either the natural form (where unambiguous) or the sentinel
//! form. Hex strings without the sentinel envelope are NOT decoded as bytes — only the
//! sentinel form preserves byte semantics through a round-trip.

use core::fmt;

use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{self, MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
};

use crate::Value;

const SENTINEL_KEY: &str = "@cbor";
const SENTINEL_BYTES: &str = "bytes";
const SENTINEL_INT: &str = "int";
const SENTINEL_MAP: &str = "map";
const SENTINEL_TAG: &str = "tag";

/// With `serde_json/arbitrary_precision` enabled, numbers are encoded as a one-entry map
/// keyed on this sentinel string and a stringified value. Any workspace member can light
/// this up; we must accept it on decode regardless of who turned the feature on.
const JSON_NUMBER_SENTINEL: &str = "$serde_json::private::Number";

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => s.serialize_unit(),
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Integer(i) => {
                if let Ok(n) = i64::try_from(*i) {
                    s.serialize_i64(n)
                } else if let Ok(n) = u64::try_from(*i) {
                    s.serialize_u64(n)
                } else {
                    let mut m = s.serialize_map(Some(2))?;
                    m.serialize_entry(SENTINEL_KEY, SENTINEL_INT)?;
                    m.serialize_entry("value", &i.to_string())?;
                    m.end()
                }
            },
            Value::Float(f) => s.serialize_f64(*f),
            Value::Bytes(b) => {
                let mut m = s.serialize_map(Some(2))?;
                m.serialize_entry(SENTINEL_KEY, SENTINEL_BYTES)?;
                m.serialize_entry("hex", &encode_hex(b))?;
                m.end()
            },
            Value::Text(t) => s.serialize_str(t),
            Value::Array(arr) => {
                let mut seq = s.serialize_seq(Some(arr.len()))?;
                for v in arr {
                    seq.serialize_element(v)?;
                }
                seq.end()
            },
            Value::Map(m) => {
                if m.iter().all(|(k, _)| matches!(k, Value::Text(_))) {
                    let mut map = s.serialize_map(Some(m.len()))?;
                    for (k, v) in m {
                        if let Value::Text(k_str) = k {
                            map.serialize_entry(k_str, v)?;
                        }
                    }
                    map.end()
                } else {
                    let mut map = s.serialize_map(Some(2))?;
                    map.serialize_entry(SENTINEL_KEY, SENTINEL_MAP)?;
                    map.serialize_entry("entries", m)?;
                    map.end()
                }
            },
            Value::Tag(t, v) => {
                let mut m = s.serialize_map(Some(3))?;
                m.serialize_entry(SENTINEL_KEY, SENTINEL_TAG)?;
                m.serialize_entry("tag", t)?;
                m.serialize_entry("value", v.as_ref())?;
                m.end()
            },
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a CBOR value (JSON-compatible representation)")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(ValueVisitor)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Integer(i128::from(v)))
    }

    fn visit_i128<E: de::Error>(self, v: i128) -> Result<Value, E> {
        Ok(Value::Integer(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Integer(i128::from(v)))
    }

    fn visit_u128<E: de::Error>(self, v: u128) -> Result<Value, E> {
        i128::try_from(v).map(Value::Integer).map_err(de::Error::custom)
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Float(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::Text(v.to_string()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::Text(v))
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Value, E> {
        Ok(Value::Bytes(v.to_vec()))
    }

    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Value, E> {
        Ok(Value::Bytes(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(elem) = seq.next_element::<Value>()? {
            out.push(elem);
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        // Pull the first key. If it's the sentinel, decode the special form. Otherwise it's a
        // plain string-keyed map and we continue collecting entries.
        let Some(first_key) = map.next_key::<String>()? else {
            return Ok(Value::Map(Vec::new()));
        };

        if first_key == SENTINEL_KEY {
            let kind: String = map.next_value()?;
            return decode_sentinel(&kind, &mut map);
        }

        if first_key == JSON_NUMBER_SENTINEL {
            let raw: String = map.next_value()?;
            return parse_json_arbitrary_number(&raw).map_err(de::Error::custom);
        }

        let first_value: Value = map.next_value()?;
        let mut entries: Vec<(Value, Value)> = Vec::with_capacity(map.size_hint().unwrap_or(0) + 1);
        entries.push((Value::Text(first_key), first_value));
        while let Some((k, v)) = map.next_entry::<String, Value>()? {
            entries.push((Value::Text(k), v));
        }
        Ok(Value::Map(entries))
    }
}

fn decode_sentinel<'de, A: MapAccess<'de>>(kind: &str, map: &mut A) -> Result<Value, A::Error> {
    match kind {
        SENTINEL_BYTES => {
            let mut hex_value: Option<String> = None;
            while let Some(k) = map.next_key::<String>()? {
                if k == "hex" {
                    hex_value = Some(map.next_value()?);
                } else {
                    let _: serde::de::IgnoredAny = map.next_value()?;
                }
            }
            let hex = hex_value.ok_or_else(|| de::Error::custom("missing 'hex' in @cbor:bytes"))?;
            decode_hex(&hex).map(Value::Bytes).map_err(de::Error::custom)
        },
        SENTINEL_INT => {
            let mut value_str: Option<String> = None;
            while let Some(k) = map.next_key::<String>()? {
                if k == "value" {
                    value_str = Some(map.next_value()?);
                } else {
                    let _: serde::de::IgnoredAny = map.next_value()?;
                }
            }
            let s = value_str.ok_or_else(|| de::Error::custom("missing 'value' in @cbor:int"))?;
            s.parse::<i128>().map(Value::Integer).map_err(de::Error::custom)
        },
        SENTINEL_MAP => {
            let mut entries: Option<Vec<(Value, Value)>> = None;
            while let Some(k) = map.next_key::<String>()? {
                if k == "entries" {
                    entries = Some(map.next_value()?);
                } else {
                    let _: serde::de::IgnoredAny = map.next_value()?;
                }
            }
            entries
                .map(Value::Map)
                .ok_or_else(|| de::Error::custom("missing 'entries' in @cbor:map"))
        },
        SENTINEL_TAG => {
            let mut tag: Option<u64> = None;
            let mut value: Option<Value> = None;
            while let Some(k) = map.next_key::<String>()? {
                match k.as_str() {
                    "tag" => tag = Some(map.next_value()?),
                    "value" => value = Some(map.next_value()?),
                    _ => {
                        let _: serde::de::IgnoredAny = map.next_value()?;
                    },
                }
            }
            let tag = tag.ok_or_else(|| de::Error::custom("missing 'tag' in @cbor:tag"))?;
            let value = value.ok_or_else(|| de::Error::custom("missing 'value' in @cbor:tag"))?;
            Ok(Value::Tag(tag, Box::new(value)))
        },
        other => Err(de::Error::custom(format!("unknown @cbor sentinel kind: {other}"))),
    }
}

/// Decode the string payload of a `$serde_json::private::Number` map. Integers prefer
/// `Value::Integer` (lossless for the i128 range); anything else falls back to `Value::Float`.
fn parse_json_arbitrary_number(raw: &str) -> Result<Value, String> {
    if let Ok(i) = raw.parse::<i128>() {
        return Ok(Value::Integer(i));
    }
    raw.parse::<f64>()
        .map(Value::Float)
        .map_err(|_| format!("invalid arbitrary_precision number: {raw}"))
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(hex_digit(b >> 4));
        out.push(hex_digit(b & 0x0f));
    }
    out
}

fn hex_digit(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + n - 10) as char,
        _ => unreachable!(),
    }
}

fn decode_hex(s: &str) -> Result<Vec<u8>, &'static str> {
    if !s.len().is_multiple_of(2) {
        return Err("odd-length hex string");
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for chunk in bytes.chunks_exact(2) {
        let hi = parse_nibble(chunk[0])?;
        let lo = parse_nibble(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn parse_nibble(b: u8) -> Result<u8, &'static str> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err("invalid hex character"),
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;

    fn json_roundtrip(v: &Value) -> Value {
        let s = serde_json::to_string(v).unwrap();
        serde_json::from_str(&s).unwrap()
    }

    #[test]
    fn primitive_round_trips() {
        for v in [
            Value::Null,
            Value::Bool(true),
            Value::Bool(false),
            Value::Integer(0),
            Value::Integer(-1),
            Value::Integer(i64::MAX.into()),
            Value::Integer(i64::MIN.into()),
            Value::Float(PI),
            Value::Text("hello world".into()),
        ] {
            assert_eq!(json_roundtrip(&v), v);
        }
    }

    #[test]
    fn integer_outside_i64_uses_sentinel() {
        let v = Value::Integer(i128::from(u64::MAX) + 1);
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("@cbor"));
        assert_eq!(json_roundtrip(&v), v);
    }

    #[test]
    fn bytes_uses_sentinel_and_round_trips() {
        let v = Value::Bytes(vec![0, 1, 2, 254, 255]);
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("@cbor"));
        assert!(json.contains("bytes"));
        assert_eq!(json_roundtrip(&v), v);
    }

    #[test]
    fn array_round_trips() {
        let v = Value::Array(vec![Value::Integer(1), Value::Text("a".into()), Value::Bool(true)]);
        assert_eq!(json_roundtrip(&v), v);
    }

    #[test]
    fn string_keyed_map_uses_native_json_object() {
        let v = Value::Map(vec![
            (Value::Text("a".into()), Value::Integer(1)),
            (Value::Text("b".into()), Value::Integer(2)),
        ]);
        let json = serde_json::to_string(&v).unwrap();
        assert!(!json.contains("@cbor"));
        assert_eq!(json_roundtrip(&v), v);
    }

    #[test]
    fn decodes_serde_json_arbitrary_precision_float() {
        // Shape produced by serde_json when its `arbitrary_precision` feature is on.
        let raw = r#"{"$serde_json::private::Number":"2.14"}"#;
        let decoded: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(decoded, Value::Float(2.14));
    }

    #[test]
    fn decodes_serde_json_arbitrary_precision_integer() {
        let raw = r#"{"$serde_json::private::Number":"42"}"#;
        let decoded: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(decoded, Value::Integer(42));
    }

    #[test]
    fn decodes_serde_json_arbitrary_precision_negative_integer() {
        let raw = r#"{"$serde_json::private::Number":"-1"}"#;
        let decoded: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(decoded, Value::Integer(-1));
    }

    #[test]
    fn non_text_keyed_map_uses_sentinel() {
        let v = Value::Map(vec![(Value::Integer(0), Value::Text("zero".into()))]);
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("@cbor"));
        assert_eq!(json_roundtrip(&v), v);
    }

    #[test]
    fn tagged_value_round_trips() {
        let v = Value::Tag(42, Box::new(Value::Text("inner".into())));
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("@cbor"));
        assert!(json.contains("tag"));
        assert_eq!(json_roundtrip(&v), v);
    }

    #[test]
    fn nested_complex_round_trips() {
        let v = crate::cbor!({
            "code" => 415u32,
            "active" => false,
            "items" => [1u32, 2u32, 3u32],
            "tagged" => Value::Tag(100, Box::new(Value::Bytes(vec![1, 2, 3]))),
        });
        assert_eq!(json_roundtrip(&v), v);
    }
}
