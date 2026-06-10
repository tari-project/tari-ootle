//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, string::String, vec::Vec};

use minicbor::{
    CborLen,
    Decoder,
    Encoder,
    data::{Tag, Type},
    decode,
    encode::{self, Write},
};

/// A dynamic CBOR value tree.
///
/// Use this for arbitrary CBOR construction/inspection and for the dynamic value paths used by
/// `IndexedValue`. Wire-format types should use `#[derive(minicbor::Encode, minicbor::Decode)]`
/// with `#[n(N)]` tags rather than going through `Value`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// CBOR integer. The CBOR spec allows the range `-2^64 ..= 2^64 - 1` which fits in `i128`.
    /// Encoding values outside `i64::MIN ..= u64::MAX` will return an error.
    Integer(i128),
    Float(f64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Tag(u64, Box<Value>),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Returns true if this value represents the CBOR encoding of a unit value (no payload).
    /// Either explicit `Value::Null` (what ciborium / `serde` produce for `()`) or `Value::Array(empty)`
    /// (what `minicbor` produces for `()` — see `impl Encode for ()`). Treating both as unit lets
    /// callers check "function returned nothing" without coupling to the encoder choice.
    pub fn is_unit(&self) -> bool {
        match self {
            Value::Null => true,
            Value::Array(items) => items.is_empty(),
            _ => false,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(b) = self { Some(*b) } else { None }
    }

    pub fn as_integer(&self) -> Option<i128> {
        if let Value::Integer(i) = self { Some(*i) } else { None }
    }

    pub fn as_float(&self) -> Option<f64> {
        if let Value::Float(f) = self { Some(*f) } else { None }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        if let Value::Bytes(b) = self { Some(b) } else { None }
    }

    pub fn as_text(&self) -> Option<&str> {
        if let Value::Text(s) = self { Some(s) } else { None }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        if let Value::Array(a) = self { Some(a) } else { None }
    }

    pub fn as_map(&self) -> Option<&[(Value, Value)]> {
        if let Value::Map(m) = self { Some(m) } else { None }
    }

    pub fn as_tag(&self) -> Option<(u64, &Value)> {
        if let Value::Tag(t, v) = self {
            Some((*t, v))
        } else {
            None
        }
    }

    /// Decode this dynamic value into a concrete minicbor-decodable type.
    ///
    /// Round-trips through `encode` + `decode`, so it's an O(N) operation in the size of the
    /// encoded form. Prefer decoding the original bytes directly when you have them.
    pub fn decoded<T: for<'b> minicbor::Decode<'b, ()>>(&self) -> Result<T, crate::BorError> {
        crate::from_value(self)
    }
}

// `From` impls for common literal types — these power the `cbor!` macro.

macro_rules! impl_from_int {
    ($($t:ty),* $(,)?) => {
        $(
            impl From<$t> for Value {
                fn from(v: $t) -> Self {
                    Value::Integer(i128::from(v))
                }
            }
        )*
    };
}

impl_from_int!(i8, i16, i32, i64, u8, u16, u32, u64);

// `usize`/`isize` are platform-width, so `i128: From<{u,i}size>` is not provided. On every
// platform the workspace targets (16/32/64-bit) the cast is lossless, but the compiler can't
// prove it generically — fall back to `as` with an explicit allow.
impl From<usize> for Value {
    fn from(v: usize) -> Self {
        #[allow(clippy::cast_lossless, clippy::cast_possible_wrap)]
        Value::Integer(v as i128)
    }
}

impl From<isize> for Value {
    fn from(v: isize) -> Self {
        #[allow(clippy::cast_lossless)]
        Value::Integer(v as i128)
    }
}

impl From<i128> for Value {
    fn from(v: i128) -> Self {
        Value::Integer(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(f64::from(v))
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(v.into())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::Bytes(v)
    }
}

impl From<&[u8]> for Value {
    fn from(v: &[u8]) -> Self {
        Value::Bytes(v.to_vec())
    }
}

impl From<()> for Value {
    fn from(_: ()) -> Self {
        Value::Null
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        match v {
            Some(t) => t.into(),
            None => Value::Null,
        }
    }
}

// `Encode<C>` is generic over the encoding context so `Value` composes with any user context.
impl<C> minicbor::Encode<C> for Value {
    fn encode<W: Write>(&self, e: &mut Encoder<W>, ctx: &mut C) -> Result<(), encode::Error<W::Error>> {
        match self {
            Value::Null => {
                e.null()?;
            },
            Value::Bool(b) => {
                e.bool(*b)?;
            },
            Value::Integer(i) => {
                if *i >= 0 {
                    let u = u64::try_from(*i)
                        .map_err(|_| encode::Error::message("Value::Integer out of CBOR range (positive)"))?;
                    e.u64(u)?;
                } else {
                    let n = i64::try_from(*i)
                        .map_err(|_| encode::Error::message("Value::Integer out of CBOR range (negative)"))?;
                    e.i64(n)?;
                }
            },
            Value::Float(f) => {
                e.f64(*f)?;
            },
            Value::Bytes(b) => {
                e.bytes(b)?;
            },
            Value::Text(s) => {
                e.str(s)?;
            },
            Value::Array(arr) => {
                e.array(arr.len() as u64)?;
                for v in arr {
                    v.encode(e, ctx)?;
                }
            },
            Value::Map(m) => {
                e.map(m.len() as u64)?;
                for (k, v) in m {
                    k.encode(e, ctx)?;
                    v.encode(e, ctx)?;
                }
            },
            Value::Tag(t, v) => {
                e.tag(Tag::new(*t))?;
                v.encode(e, ctx)?;
            },
        }
        Ok(())
    }
}

/// Maximum container-nesting depth accepted when decoding a dynamic [`Value`] tree.
///
/// The decode is recursive (one stack frame per nested array/map/tag), and the input is untrusted
/// — roughly one byte of input buys one level of nesting, so without a bound a tiny payload can
/// drive the decoder into a stack overflow (a process abort, not a catchable error). This caps the
/// recursion so over-nested input fails as a clean `Err` instead.
///
/// Set just above the semantic `MAX_VISITOR_DEPTH` (50) enforced on the materialised tree in
/// `engine_types::indexed_value`: high enough that this never rejects input downstream validation
/// would accept, low enough that the worst-case recursion stays a small fraction of the stack on
/// the threads that run untrusted decode (e.g. tokio's 2 MiB workers).
pub const MAX_DECODE_DEPTH: usize = 64;

impl<'b, C> minicbor::Decode<'b, C> for Value {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, decode::Error> {
        decode_value(d, ctx, 0)
    }
}

fn decode_value<'b, C>(d: &mut Decoder<'b>, ctx: &mut C, depth: usize) -> Result<Value, decode::Error> {
    if depth >= MAX_DECODE_DEPTH {
        return Err(decode::Error::message("maximum CBOR nesting depth exceeded"));
    }
    match d.datatype()? {
        Type::Null => {
            d.null()?;
            Ok(Value::Null)
        },
        Type::Undefined => {
            d.undefined()?;
            Ok(Value::Null)
        },
        Type::Bool => Ok(Value::Bool(d.bool()?)),
        Type::U8 | Type::U16 | Type::U32 | Type::U64 => Ok(Value::Integer(i128::from(d.u64()?))),
        Type::I8 | Type::I16 | Type::I32 | Type::I64 => Ok(Value::Integer(i128::from(d.i64()?))),
        Type::Int => Ok(Value::Integer(i128::from(d.int()?))),
        Type::F16 | Type::F32 | Type::F64 => Ok(Value::Float(d.f64()?)),
        Type::Bytes => Ok(Value::Bytes(d.bytes()?.to_vec())),
        Type::BytesIndef => {
            let mut out = Vec::new();
            for chunk in d.bytes_iter()? {
                out.extend_from_slice(chunk?);
            }
            Ok(Value::Bytes(out))
        },
        Type::String => Ok(Value::Text(d.str()?.to_string())),
        Type::StringIndef => {
            let mut out = String::new();
            for chunk in d.str_iter()? {
                out.push_str(chunk?);
            }
            Ok(Value::Text(out))
        },
        Type::Array => {
            let len = d.array()?;
            Ok(Value::Array(decode_array(d, len, ctx, depth)?))
        },
        Type::ArrayIndef => {
            let _ = d.array()?;
            Ok(Value::Array(decode_array(d, None, ctx, depth)?))
        },
        Type::Map => {
            let len = d.map()?;
            Ok(Value::Map(decode_map(d, len, ctx, depth)?))
        },
        Type::MapIndef => {
            let _ = d.map()?;
            Ok(Value::Map(decode_map(d, None, ctx, depth)?))
        },
        Type::Tag => {
            let tag: u64 = d.tag()?.into();
            let inner = decode_value(d, ctx, depth + 1)?;
            Ok(Value::Tag(tag, Box::new(inner)))
        },
        other => Err(decode::Error::message("unsupported CBOR datatype").with_message(other)),
    }
}

/// `CborLen` for the dynamic `Value` tree.
///
/// Computes length recursively over the structure. For static types prefer `#[derive(CborLen)]`
/// which is O(1) per field.
impl<C> CborLen<C> for Value {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        match self {
            Value::Null | Value::Bool(_) => 1,
            Value::Integer(i) => {
                if *i >= 0 {
                    u64::try_from(*i).map(|u| u.cbor_len(ctx)).unwrap_or(9)
                } else {
                    i64::try_from(*i).map(|n| n.cbor_len(ctx)).unwrap_or(9)
                }
            },
            Value::Float(_) => 9,
            Value::Bytes(b) => {
                let n = b.len();
                n.cbor_len(ctx) + n
            },
            Value::Text(s) => {
                let n = s.len();
                n.cbor_len(ctx) + n
            },
            Value::Array(arr) => {
                let n = arr.len();
                n.cbor_len(ctx) + arr.iter().map(|v| v.cbor_len(ctx)).sum::<usize>()
            },
            Value::Map(m) => {
                let n = m.len();
                n.cbor_len(ctx) + m.iter().map(|(k, v)| k.cbor_len(ctx) + v.cbor_len(ctx)).sum::<usize>()
            },
            Value::Tag(t, v) => t.cbor_len(ctx) + v.cbor_len(ctx),
        }
    }
}

#[cfg(not(feature = "std"))]
use alloc::string::ToString;

fn decode_array<'b, C>(
    d: &mut Decoder<'b>,
    len: Option<u64>,
    ctx: &mut C,
    depth: usize,
) -> Result<Vec<Value>, decode::Error> {
    let mut out = match len {
        Some(n) => Vec::with_capacity(n.min(64) as usize),
        None => Vec::new(),
    };
    match len {
        Some(n) => {
            for _ in 0..n {
                out.push(decode_value(d, ctx, depth + 1)?);
            }
        },
        None => loop {
            if matches!(d.datatype()?, Type::Break) {
                d.skip()?;
                break;
            }
            out.push(decode_value(d, ctx, depth + 1)?);
        },
    }
    Ok(out)
}

fn decode_map<'b, C>(
    d: &mut Decoder<'b>,
    len: Option<u64>,
    ctx: &mut C,
    depth: usize,
) -> Result<Vec<(Value, Value)>, decode::Error> {
    let mut out = match len {
        Some(n) => Vec::with_capacity(n.min(64) as usize),
        None => Vec::new(),
    };
    match len {
        Some(n) => {
            for _ in 0..n {
                let k = decode_value(d, ctx, depth + 1)?;
                let v = decode_value(d, ctx, depth + 1)?;
                out.push((k, v));
            }
        },
        None => loop {
            if matches!(d.datatype()?, Type::Break) {
                d.skip()?;
                break;
            }
            let k = decode_value(d, ctx, depth + 1)?;
            let v = decode_value(d, ctx, depth + 1)?;
            out.push((k, v));
        },
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(v: &Value) -> Value {
        let bytes = minicbor::to_vec(v).unwrap();
        assert_eq!(bytes.len(), minicbor::len(v), "CborLen mismatch");
        minicbor::decode::<Value>(&bytes).unwrap()
    }

    #[test]
    fn null_roundtrip() {
        assert_eq!(roundtrip(&Value::Null), Value::Null);
    }

    #[test]
    fn bool_roundtrip() {
        assert_eq!(roundtrip(&Value::Bool(true)), Value::Bool(true));
        assert_eq!(roundtrip(&Value::Bool(false)), Value::Bool(false));
    }

    #[test]
    fn integer_roundtrip() {
        for v in [
            0i128,
            1,
            -1,
            42,
            -42,
            i128::from(i64::MAX),
            i128::from(i64::MIN),
            i128::from(u64::MAX),
        ] {
            assert_eq!(roundtrip(&Value::Integer(v)), Value::Integer(v), "value: {v}");
        }
    }

    #[test]
    fn float_roundtrip() {
        for v in [0.0f64, 1.5, -1.5, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(roundtrip(&Value::Float(v)), Value::Float(v));
        }
    }

    #[test]
    fn bytes_roundtrip() {
        assert_eq!(roundtrip(&Value::Bytes(vec![])), Value::Bytes(vec![]));
        assert_eq!(
            roundtrip(&Value::Bytes(vec![1, 2, 3, 255])),
            Value::Bytes(vec![1, 2, 3, 255])
        );
    }

    #[test]
    fn text_roundtrip() {
        assert_eq!(roundtrip(&Value::Text(String::new())), Value::Text(String::new()));
        assert_eq!(
            roundtrip(&Value::Text("héllo".to_string())),
            Value::Text("héllo".to_string())
        );
    }

    #[test]
    fn array_roundtrip() {
        let v = Value::Array(vec![Value::Integer(1), Value::Text("a".into()), Value::Bool(true)]);
        assert_eq!(roundtrip(&v), v);
    }

    #[test]
    fn nested_array_roundtrip() {
        let v = Value::Array(vec![
            Value::Array(vec![Value::Integer(1), Value::Integer(2)]),
            Value::Null,
        ]);
        assert_eq!(roundtrip(&v), v);
    }

    #[test]
    fn map_roundtrip() {
        let v = Value::Map(vec![
            (Value::Text("a".into()), Value::Integer(1)),
            (Value::Integer(0), Value::Bool(false)),
        ]);
        assert_eq!(roundtrip(&v), v);
    }

    #[test]
    fn tagged_roundtrip() {
        let v = Value::Tag(42, Box::new(Value::Text("inner".into())));
        assert_eq!(roundtrip(&v), v);
    }

    #[test]
    fn nested_tagged_roundtrip() {
        let v = Value::Tag(
            100,
            Box::new(Value::Map(vec![(
                Value::Text("k".into()),
                Value::Tag(200, Box::new(Value::Integer(7))),
            )])),
        );
        assert_eq!(roundtrip(&v), v);
    }

    #[test]
    fn deeply_nested_within_limit_roundtrips() {
        // Comfortably below MAX_DECODE_DEPTH: legitimately nested input must still decode.
        let depth = MAX_DECODE_DEPTH / 2;
        let mut v = Value::Integer(1);
        for _ in 0..depth {
            v = Value::Array(vec![v]);
        }
        assert_eq!(roundtrip(&v), v);
    }

    // The decode is recursive, so without a depth bound a tiny untrusted payload (≈1 byte per
    // nesting level) overflows the stack and aborts the process. These assert that over-nested
    // input of each container kind fails as a clean `Err` instead. Each chain is far longer than
    // any real stack could survive unbounded, so a regression would abort the test binary.

    #[test]
    fn deeply_nested_array_errors_instead_of_overflow() {
        let bytes = vec![0x81u8; 100_000]; // chain of CBOR "array of 1"
        assert!(minicbor::decode::<Value>(&bytes).is_err());
    }

    #[test]
    fn deeply_nested_indefinite_array_errors_instead_of_overflow() {
        let bytes = vec![0x9fu8; 100_000]; // chain of CBOR "indefinite-length array"
        assert!(minicbor::decode::<Value>(&bytes).is_err());
    }

    #[test]
    fn deeply_nested_map_errors_instead_of_overflow() {
        let bytes = vec![0xa1u8; 100_000]; // chain of CBOR "map of 1" (recurses on each key)
        assert!(minicbor::decode::<Value>(&bytes).is_err());
    }

    #[test]
    fn deeply_nested_tag_errors_instead_of_overflow() {
        let bytes = vec![0xc0u8; 100_000]; // chain of CBOR "tag"
        assert!(minicbor::decode::<Value>(&bytes).is_err());
    }
}
