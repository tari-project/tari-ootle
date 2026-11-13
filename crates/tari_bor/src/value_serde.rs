//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
//
//! Serde `Serialize` / `Deserialize` for [`Value`].
//!
//! Used for JSON and for the [`crate::serde_codec`] CBOR route, which stores serde types holding a
//! `Value`. Consensus encoding goes through minicbor's `Encode`/`Decode` and never through serde.
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
//! | `Map(m)` where every key is `Text` and none is reserved | object |
//!
//! Variants without a natural JSON shape are emitted using a sentinel object keyed on
//! `"@cbor"`:
//!
//! | Variant | JSON |
//! |---------|------|
//! | `Bytes(b)` | `{ "@cbor": "bytes", "hex": "ab12.." }` |
//! | `Integer(i)` (outside i64/u64 range) | `{ "@cbor": "int", "value": "12345" }` |
//! | `Map(m)` with a non-text or reserved key | `{ "@cbor": "map", "entries": [[k, v], ...] }` |
//! | `Tag(t, v)` | `{ "@cbor": "tag", "tag": N, "value": v }` |
//!
//! The reserved keys are `"@cbor"` and `"$serde_json::private::Number"`, the two first keys the
//! decoder reads as a special form.
//!
//! Deserialisation accepts either the natural form (where unambiguous) or the sentinel
//! form. Hex strings without the sentinel envelope are NOT decoded as bytes — only the
//! sentinel form preserves byte semantics through a round-trip.

#[cfg(not(feature = "std"))]
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;

use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{self, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
};

use crate::{MAX_DECODE_DEPTH, Value};

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
            // `serialize_unit` is format-defined: `serde_codec` writes `()` as an empty array.
            Value::Null => s.serialize_none(),
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
                if m.iter().all(|(k, _)| is_native_object_key(k)) {
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

/// The decoder reads an object whose first key is reserved as a special form, and any key may come
/// first once the JSON passes through a key-sorting map such as `serde_json::Value`. A map holding a
/// reserved key anywhere therefore goes through the `@cbor:map` envelope.
fn is_native_object_key(key: &Value) -> bool {
    matches!(key, Value::Text(k) if k != SENTINEL_KEY && k != JSON_NUMBER_SENTINEL)
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ValueSeed { depth: 0 }.deserialize(d)
    }
}

/// Depth-bounded seed for [`Value`], enforcing [`MAX_DECODE_DEPTH`] identically to
/// `minicbor::Decode for Value` and [`crate::check_value_form`].
#[derive(Clone, Copy)]
struct ValueSeed {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for ValueSeed {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        if self.depth >= MAX_DECODE_DEPTH {
            return Err(de::Error::custom("maximum CBOR nesting depth exceeded"));
        }
        d.deserialize_any(ValueVisitor { depth: self.depth })
    }
}

struct ValueVisitor {
    depth: usize,
}

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
        ValueSeed { depth: self.depth }.deserialize(d)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        ValueSeed { depth: self.depth }.deserialize(d)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Integer(i128::from(v)))
    }

    fn visit_i128<E: de::Error>(self, v: i128) -> Result<Value, E> {
        Value::integer(v).ok_or_else(|| de::Error::custom("Value::Integer out of CBOR range"))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Integer(i128::from(v)))
    }

    fn visit_u128<E: de::Error>(self, v: u128) -> Result<Value, E> {
        i128::try_from(v)
            .ok()
            .and_then(Value::integer)
            .ok_or_else(|| de::Error::custom("Value::Integer out of CBOR range"))
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
        let mut out = Vec::with_capacity(cautious_capacity(seq.size_hint()));
        while let Some(elem) = seq.next_element_seed(ValueSeed { depth: self.depth + 1 })? {
            out.push(elem);
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        // Pull the first key. If it's a reserved text sentinel, decode the special form; otherwise
        // collect entries directly (supporting both text-keyed JSON objects and arbitrary-keyed CBOR
        // maps via `serde_codec`).
        let entry_seed = ValueSeed { depth: self.depth + 1 };
        let Some(first_key) = map.next_key_seed(entry_seed)? else {
            return Ok(Value::Map(Vec::new()));
        };

        if let Value::Text(ref key_str) = first_key {
            if key_str == SENTINEL_KEY {
                let kind: String = map.next_value()?;
                return decode_sentinel(&kind, &mut map, self.depth);
            }

            if key_str == JSON_NUMBER_SENTINEL {
                let raw: String = map.next_value()?;
                if map
                    .next_key_seed(BoundedIgnoredAny { depth: self.depth + 1 })?
                    .is_some()
                {
                    return Err(de::Error::custom(
                        "unexpected extra key in $serde_json::private::Number sentinel",
                    ));
                }
                return parse_json_arbitrary_number(&raw).map_err(de::Error::custom);
            }
        }

        let first_value = map.next_value_seed(entry_seed)?;
        let mut entries: Vec<(Value, Value)> = Vec::with_capacity(cautious_capacity(map.size_hint()) + 1);
        entries.push((first_key, first_value));
        while let Some((k, v)) = map.next_entry_seed(entry_seed, entry_seed)? {
            entries.push((k, v));
        }
        Ok(Value::Map(entries))
    }
}

/// `size_hint` from a binary deserializer is the input's own length header, so it only seeds the capacity up to
/// [`MAX_PREALLOC`](crate::adapters::MAX_PREALLOC); the collection grows as elements actually decode.
fn cautious_capacity(hint: Option<usize>) -> usize {
    hint.unwrap_or(0).min(crate::adapters::MAX_PREALLOC as usize)
}

fn decode_sentinel<'de, A: MapAccess<'de>>(kind: &str, map: &mut A, depth: usize) -> Result<Value, A::Error> {
    match kind {
        SENTINEL_BYTES => {
            let mut hex_value: Option<String> = None;
            while let Some(k) = map.next_key::<String>()? {
                if k == "hex" {
                    hex_value = Some(map.next_value()?);
                } else {
                    map.next_value_seed(BoundedIgnoredAny { depth: depth + 1 })?;
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
                    map.next_value_seed(BoundedIgnoredAny { depth: depth + 1 })?;
                }
            }
            let s = value_str.ok_or_else(|| de::Error::custom("missing 'value' in @cbor:int"))?;
            let i = s.parse::<i128>().map_err(de::Error::custom)?;
            Value::integer(i).ok_or_else(|| de::Error::custom("Value::Integer out of CBOR range"))
        },
        SENTINEL_MAP => {
            let mut entries: Option<Vec<(Value, Value)>> = None;
            while let Some(k) = map.next_key::<String>()? {
                if k == "entries" {
                    entries = Some(map.next_value_seed(MapEntriesSeed { entry_depth: depth + 1 })?);
                } else {
                    map.next_value_seed(BoundedIgnoredAny { depth: depth + 1 })?;
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
                    "value" => value = Some(map.next_value_seed(ValueSeed { depth: depth + 1 })?),
                    _ => {
                        map.next_value_seed(BoundedIgnoredAny { depth: depth + 1 })?;
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

/// Decodes the `"entries"` sequence of a `@cbor:map` envelope, charging each key and value at
/// `entry_depth` (i.e. `map_depth + 1`) so that the envelope framing does not consume extra depth
/// relative to the native CBOR map encoding.
#[derive(Clone, Copy)]
struct MapEntriesSeed {
    entry_depth: usize,
}

impl<'de> DeserializeSeed<'de> for MapEntriesSeed {
    type Value = Vec<(Value, Value)>;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for MapEntriesSeed {
    type Value = Vec<(Value, Value)>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a sequence of [key, value] pairs")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut out = Vec::with_capacity(cautious_capacity(seq.size_hint()));
        let pair_seed = MapEntryPairSeed {
            entry_depth: self.entry_depth,
        };
        while let Some(pair) = seq.next_element_seed(pair_seed)? {
            out.push(pair);
        }
        Ok(out)
    }
}

#[derive(Clone, Copy)]
struct MapEntryPairSeed {
    entry_depth: usize,
}

impl<'de> DeserializeSeed<'de> for MapEntryPairSeed {
    type Value = (Value, Value);

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_tuple(2, self)
    }
}

impl<'de> Visitor<'de> for MapEntryPairSeed {
    type Value = (Value, Value);

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a [key, value] pair")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let key = seq
            .next_element_seed(ValueSeed {
                depth: self.entry_depth,
            })?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let val = seq
            .next_element_seed(ValueSeed {
                depth: self.entry_depth,
            })?
            .ok_or_else(|| de::Error::invalid_length(1, &self))?;
        if seq
            .next_element_seed(BoundedIgnoredAny {
                depth: self.entry_depth,
            })?
            .is_some()
        {
            return Err(de::Error::invalid_length(3, &self));
        }
        Ok((key, val))
    }
}

/// Depth-bounded counterpart to [`serde::de::IgnoredAny`] for skipping unrecognized fields inside
/// sentinel maps without allowing unbounded native stack recursion.
#[derive(Clone, Copy)]
struct BoundedIgnoredAny {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for BoundedIgnoredAny {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        if self.depth >= MAX_DECODE_DEPTH {
            return Err(de::Error::custom("maximum CBOR nesting depth exceeded"));
        }
        d.deserialize_ignored_any(self)
    }
}

impl<'de> Visitor<'de> for BoundedIgnoredAny {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any value")
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_i128<E: de::Error>(self, _: i128) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u128<E: de::Error>(self, _: u128) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_bytes<E: de::Error>(self, _: &[u8]) -> Result<(), E> {
        Ok(())
    }

    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        BoundedIgnoredAny { depth: self.depth }.deserialize(d)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        BoundedIgnoredAny { depth: self.depth }.deserialize(d)
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let next = BoundedIgnoredAny { depth: self.depth + 1 };
        while seq.next_element_seed(next)?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let next = BoundedIgnoredAny { depth: self.depth + 1 };
        while map.next_entry_seed(next, next)?.is_some() {}
        Ok(())
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<(), A::Error> {
        let ((), variant) = data.variant_seed(BoundedIgnoredAny { depth: self.depth })?;
        variant.newtype_variant_seed(BoundedIgnoredAny { depth: self.depth + 1 })
    }
}

enum JsonNumberKind {
    Integer,
    Float,
}

/// Validates RFC 8259 JSON number syntax (`-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`) and
/// classifies the token as an integer or float.
fn classify_json_number(raw: &str) -> Option<JsonNumberKind> {
    let bytes = raw.as_bytes();
    let rest = bytes.strip_prefix(b"-").unwrap_or(bytes);
    let after_int = match rest.first()? {
        b'0' => &rest[1..],
        b'1'..=b'9' => {
            let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            &rest[digits..]
        },
        _ => return None,
    };
    if after_int.is_empty() {
        return Some(JsonNumberKind::Integer);
    }
    let after_frac = if let Some(frac) = after_int.strip_prefix(b".") {
        let digits = frac.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        &frac[digits..]
    } else {
        after_int
    };
    let after_exp = if let Some(exp) = after_frac.strip_prefix(b"e").or_else(|| after_frac.strip_prefix(b"E")) {
        let exp = exp.strip_prefix(b"+").or_else(|| exp.strip_prefix(b"-")).unwrap_or(exp);
        let digits = exp.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        &exp[digits..]
    } else {
        after_frac
    };
    if after_exp.is_empty() {
        Some(JsonNumberKind::Float)
    } else {
        None
    }
}

/// Decode the string payload of a `$serde_json::private::Number` map. Integer tokens must fit in
/// the CBOR integer range (never coercing to `f64`), and float tokens must be finite JSON numbers.
fn parse_json_arbitrary_number(raw: &str) -> Result<Value, String> {
    match classify_json_number(raw) {
        Some(JsonNumberKind::Integer) => {
            let i = raw
                .parse::<i128>()
                .map_err(|_| format!("integer out of CBOR range: {raw}"))?;
            Value::integer(i).ok_or_else(|| format!("integer out of CBOR range: {raw}"))
        },
        Some(JsonNumberKind::Float) => {
            let f = raw
                .parse::<f64>()
                .map_err(|_| format!("invalid arbitrary_precision number: {raw}"))?;
            if f.is_finite() {
                Ok(Value::Float(f))
            } else {
                Err(format!("float out of finite f64 range: {raw}"))
            }
        },
        None => Err(format!("invalid arbitrary_precision number: {raw}")),
    }
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
    fn integers_outside_i64_but_inside_the_cbor_range_round_trip() {
        for v in [
            Value::Integer(i128::from(u64::MAX)),
            Value::Integer(i128::from(i64::MAX) + 1),
        ] {
            assert_eq!(json_roundtrip(&v), v);
        }
    }

    // `Value::Integer` is constructible with any `i128`, so serialization stays total for one
    // outside [`INTEGER_RANGE`] and writes the sentinel. Reading it back would rebuild a value no
    // encoder can write, so every route into the type refuses it.
    #[test]
    fn an_integer_outside_the_cbor_range_is_refused_on_the_way_back() {
        let v = Value::Integer(i128::from(u64::MAX) + 1);
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("@cbor"));
        assert!(serde_json::from_str::<Value>(&json).is_err());
    }

    #[test]
    fn the_serde_decoder_rejects_a_huge_length_header_without_preallocating() {
        // An array header claiming u64::MAX elements, followed by none.
        const HUGE_ARRAY: &[u8] = &[0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        // A map header claiming u64::MAX entries, followed by one entry {"a": 1}: the visitor sizes the map after
        // reading the first entry.
        const HUGE_MAP: &[u8] = &[0xbb, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x61, b'a', 0x01];
        assert!(crate::serde_codec::from_slice::<Value>(HUGE_ARRAY).is_err());
        assert!(crate::serde_codec::from_slice::<Value>(HUGE_MAP).is_err());
    }

    // The CBOR serde route reaches `Value` through `deserialize_any`, which hands a 64-bit negative
    // to `visit_i128` rather than through the minicbor decoder.
    #[test]
    fn the_serde_decoder_refuses_an_integer_below_i64_min() {
        const NEGATIVE_BELOW_I64_MIN: [u8; 9] = [0x3b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        assert!(crate::serde_codec::from_slice::<Value>(&NEGATIVE_BELOW_I64_MIN).is_err());
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

    fn text_map(entries: &[(&str, Value)]) -> Value {
        Value::Map(
            entries
                .iter()
                .map(|(k, v)| (Value::Text((*k).into()), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn text_keyed_maps_holding_a_reserved_key_round_trip() {
        let maps = [
            text_map(&[
                (SENTINEL_KEY, Value::Text("hello".into())),
                ("symbol", Value::Text("X".into())),
            ]),
            text_map(&[
                (SENTINEL_KEY, Value::Text(SENTINEL_BYTES.into())),
                ("hex", Value::Text("c0ffee".into())),
                ("symbol", Value::Text("X".into())),
            ]),
            text_map(&[
                (SENTINEL_KEY, Value::Text(SENTINEL_MAP.into())),
                ("entries", Value::Array(vec![])),
            ]),
            text_map(&[(JSON_NUMBER_SENTINEL, Value::Text("42".into()))]),
            text_map(&[("a", Value::Integer(1)), (SENTINEL_KEY, Value::Text("hello".into()))]),
            text_map(&[(
                "outer",
                text_map(&[
                    (SENTINEL_KEY, Value::Text(SENTINEL_TAG.into())),
                    ("tag", Value::Integer(1)),
                ]),
            )]),
        ];
        for v in maps {
            let json = serde_json::to_string(&v).unwrap();
            assert!(json.contains(r#""@cbor":"map""#), "{json}");
            assert_eq!(json_roundtrip(&v), v, "{json}");
        }
    }

    // `serde_json::Value` sorts object keys, so a reserved key that is not first in the CBOR map
    // becomes the first key once the JSON passes through it.
    #[test]
    fn a_reserved_key_survives_an_intermediate_json_value() {
        let v = text_map(&[("a", Value::Integer(1)), (SENTINEL_KEY, Value::Text("hello".into()))]);
        let json: serde_json::Value = serde_json::to_value(&v).unwrap();
        assert_eq!(serde_json::from_value::<Value>(json).unwrap(), v);
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

    #[test]
    fn null_round_trips_through_the_serde_cbor_codec() {
        let v = crate::cbor!({
            "missing" => null,
            "items" => [null, 1u32],
        });
        let bytes = crate::serde_codec::to_vec(&v).unwrap();
        assert_eq!(crate::serde_codec::from_slice::<Value>(&bytes).unwrap(), v);
    }

    #[test]
    fn serde_decoder_enforces_max_decode_depth() {
        fn build_nested(levels: usize, wrap: impl Fn(Value) -> Value) -> Value {
            let mut v = Value::Integer(0);
            for _ in 0..levels {
                v = wrap(v);
            }
            v
        }

        let max = MAX_DECODE_DEPTH;
        let json_wrappers: [&dyn Fn(Value) -> Value; 3] = [
            &|inner| Value::Array(vec![inner]),
            &|inner| Value::Map(vec![(Value::Text("k".into()), inner)]),
            &|inner| Value::Tag(42, Box::new(inner)),
        ];

        for wrap in json_wrappers {
            // One below MAX_DECODE_DEPTH must round-trip through JSON and RawCbor.
            let valid = build_nested(max - 1, wrap);
            let valid_json = serde_json::to_string(&valid).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&valid_json).unwrap(), valid);
            let raw: crate::RawCbor = serde_json::from_str(&valid_json).unwrap();
            assert_eq!(raw.to_value().unwrap(), valid);
            assert_eq!(serde_json::to_string(&raw).unwrap(), valid_json);
            assert!(crate::check_value_form(raw.as_bytes()).is_ok());

            // At MAX_DECODE_DEPTH (still below serde_json's 128-level parser limit),
            // serde deserialization must refuse to build a Value or RawCbor that cannot decode.
            let too_deep = build_nested(max, wrap);
            let too_deep_json = serde_json::to_string(&too_deep).unwrap();
            assert!(serde_json::from_str::<Value>(&too_deep_json).is_err());
            assert!(serde_json::from_str::<crate::RawCbor>(&too_deep_json).is_err());
        }

        // Across `serde_codec` (including `@cbor:map`, which uses 3 container frames per CBOR map
        // level and therefore hits serde_json's 128-token parser cap before 63 levels in text JSON),
        // every wrapper succeeds at `max - 1` and is rejected at `max`.
        let all_wrappers: [&dyn Fn(Value) -> Value; 4] = [
            &|inner| Value::Array(vec![inner]),
            &|inner| Value::Map(vec![(Value::Text("k".into()), inner)]),
            &|inner| Value::Map(vec![(Value::Integer(0), inner)]),
            &|inner| Value::Tag(42, Box::new(inner)),
        ];
        for wrap in all_wrappers {
            let valid = build_nested(max - 1, wrap);
            let valid_cbor = crate::serde_codec::to_vec(&valid).unwrap();
            assert_eq!(crate::serde_codec::from_slice::<Value>(&valid_cbor).unwrap(), valid);
            let raw = crate::serde_codec::from_slice::<crate::RawCbor>(&valid_cbor).unwrap();
            assert_eq!(raw.to_value().unwrap(), valid);

            let too_deep = build_nested(max, wrap);
            let too_deep_cbor = crate::serde_codec::to_vec(&too_deep).unwrap();
            assert!(crate::serde_codec::from_slice::<Value>(&too_deep_cbor).is_err());
            assert!(crate::serde_codec::from_slice::<crate::RawCbor>(&too_deep_cbor).is_err());
        }

        // A 100,000-level CBOR array bomb via `serde_codec::from_slice::<Value>` must fail
        // cleanly at MAX_DECODE_DEPTH without overflowing a 2 MiB thread stack.
        let rejected = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let mut bomb = vec![0x81u8; 100_000];
                bomb.push(0x00);
                crate::serde_codec::from_slice::<Value>(&bomb).is_err()
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(rejected);
    }

    #[test]
    fn json_number_sentinel_consumes_map_end_and_rejects_extra_entries() {
        // Valid 1-entry indefinite-length CBOR map: {_ "$serde_json::private::Number": "42" }.
        let mut indef_map = vec![0xbf];
        indef_map.extend_from_slice(&crate::encode(&JSON_NUMBER_SENTINEL).unwrap());
        indef_map.extend_from_slice(&crate::encode(&"42").unwrap());
        indef_map.push(0xff);
        assert_eq!(
            crate::serde_codec::from_slice::<Value>(&indef_map).unwrap(),
            Value::Integer(42)
        );

        // Multi-key JSON value starting with the sentinel must be rejected.
        let extra_json = serde_json::json!({
            JSON_NUMBER_SENTINEL: "42",
            "extra": 1,
        });
        assert!(serde_json::from_value::<Value>(extra_json).is_err());

        // Truncated 3-element CBOR array containing a 2-entry map whose second entry would
        // otherwise leak out into the enclosing sequence.
        let mut smuggled = vec![0x83, 0xa2];
        smuggled.extend_from_slice(&crate::encode(&JSON_NUMBER_SENTINEL).unwrap());
        smuggled.extend_from_slice(&crate::encode(&"42").unwrap());
        smuggled.extend_from_slice(&crate::encode(&"injected").unwrap());
        smuggled.push(0x18);
        smuggled.push(99);
        assert!(crate::serde_codec::from_slice::<Value>(&smuggled).is_err());
    }

    #[test]
    fn json_arbitrary_precision_rejects_out_of_range_integers_and_non_finite_floats() {
        for bad in [
            "18446744073709551616",
            "170141183460469231731687303715884105728",
            "-170141183460469231731687303715884105729",
            "1e400",
            "-1e400",
            "NaN",
            "inf",
            "-Infinity",
            "+42",
            "+1.5",
            "01",
            "1.",
            ".5",
            "",
        ] {
            let payload = serde_json::json!({ JSON_NUMBER_SENTINEL: bad });
            assert!(
                serde_json::from_value::<Value>(payload).is_err(),
                "expected error for {bad:?}"
            );
        }

        // Valid scientific-notation and zero numbers continue to decode cleanly.
        assert_eq!(
            serde_json::from_value::<Value>(serde_json::json!({ JSON_NUMBER_SENTINEL: "0" })).unwrap(),
            Value::Integer(0)
        );
        assert_eq!(
            serde_json::from_value::<Value>(serde_json::json!({ JSON_NUMBER_SENTINEL: "1.25e2" })).unwrap(),
            Value::Float(125.0)
        );
    }

    #[test]
    fn native_non_text_cbor_maps_decode_into_value_via_serde_codec() {
        let native_cbor = crate::encode(&Value::Map(vec![
            (Value::Integer(0), Value::Text("zero".into())),
            (Value::Bytes(vec![1, 2]), Value::Bool(true)),
        ]))
        .unwrap();
        let decoded = crate::serde_codec::from_slice::<Value>(&native_cbor).unwrap();
        assert_eq!(
            decoded,
            Value::Map(vec![
                (Value::Integer(0), Value::Text("zero".into())),
                (Value::Bytes(vec![1, 2]), Value::Bool(true)),
            ])
        );
    }
}
