//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Local fork of [`minicbor-serde`](https://docs.rs/minicbor-serde) with `u128` / `i128` support
//! added on both the encode and decode sides.
//!
//! Upstream's `Serializer` and `Deserializer` don't implement `serialize_u128`/`serialize_i128`
//! or `deserialize_u128`/`deserialize_i128`, so any serde-bridged struct that names a `u128` or
//! `i128` field fails to round-trip. The relevant upstream change is in flight at
//! <https://github.com/twittner/minicbor/pull/63>; when it lands (and a `minicbor-serde` release
//! picks it up), this module can be deleted and call sites switched back to `minicbor_serde`.
//!
//! The wire format is intentionally identical to `minicbor-serde`'s for every type it already
//! supports — only the 128-bit integer arms are new. Those use the RFC 8949 §3.4.3 bignum form:
//!   - `u128`  → tag `2`  ("positive bignum") + bstr of big-endian magnitude (leading zeros stripped; a single `0x00`
//!     byte for the value `0`).
//!   - `i128`  → tag `2` for non-negative, tag `3` ("negative bignum", encoding `-1 - n`) for negative values, payload
//!     as above.
//!
//! Code structure mirrors upstream so the eventual diff to drop this fork is mechanical.

#![cfg(feature = "serde")]

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
use core::fmt;

use minicbor::{
    Decoder,
    Encoder,
    data::{Tag, Type},
    decode,
    encode::{self, Write},
};
use serde::{
    Deserialize,
    Serialize,
    de::{self, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor},
    ser::{
        self,
        SerializeMap,
        SerializeSeq,
        SerializeStruct,
        SerializeStructVariant,
        SerializeTuple,
        SerializeTupleStruct,
        SerializeTupleVariant,
    },
};

const TAG_POSITIVE_BIGNUM: u64 = 2;
const TAG_NEGATIVE_BIGNUM: u64 = 3;
const BREAK: u8 = 0xff;

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug)]
pub struct DecodeError(decode::Error);

#[derive(Debug)]
pub struct EncodeError<E>(encode::Error<E>);

impl<E> From<encode::Error<E>> for EncodeError<E> {
    fn from(e: encode::Error<E>) -> Self {
        Self(e)
    }
}

impl From<decode::Error> for DecodeError {
    fn from(e: decode::Error) -> Self {
        Self(e)
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<E: fmt::Display> fmt::Display for EncodeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl core::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.0.source()
    }
}

impl<E: core::error::Error + 'static> core::error::Error for EncodeError<E> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.0.source()
    }
}

impl<E: core::error::Error + 'static> ser::Error for EncodeError<E> {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self(encode::Error::message(msg))
    }
}

impl de::Error for DecodeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self(decode::Error::message(msg))
    }
}

// ============================================================================
// Top-level helpers
// ============================================================================

pub fn to_vec<T: Serialize>(val: T) -> Result<Vec<u8>, EncodeError<core::convert::Infallible>> {
    let mut v = Vec::new();
    val.serialize(&mut Serializer::new(&mut v))?;
    Ok(v)
}

pub fn from_slice<'de, T: Deserialize<'de>>(b: &'de [u8]) -> Result<T, DecodeError> {
    T::deserialize(&mut Deserializer::new(b))
}

// ============================================================================
// 128-bit helpers (the reason this fork exists)
// ============================================================================

fn encode_u128<W: Write>(e: &mut Encoder<W>, v: u128) -> Result<(), encode::Error<W::Error>> {
    let bytes = v.to_be_bytes();
    let first = bytes.iter().position(|&b| b != 0).unwrap_or(15);
    e.tag(Tag::new(TAG_POSITIVE_BIGNUM))?;
    e.bytes(&bytes[first..])?;
    Ok(())
}

fn encode_i128<W: Write>(e: &mut Encoder<W>, v: i128) -> Result<(), encode::Error<W::Error>> {
    if v >= 0 {
        return encode_u128(e, v as u128);
    }
    // RFC 8949 §3.4.3: negative bignum encodes -1 - n. So for v < 0, the encoded magnitude is
    // (-1 - v) as an unsigned bignum.
    let magnitude = (-1i128 - v) as u128;
    let bytes = magnitude.to_be_bytes();
    let first = bytes.iter().position(|&b| b != 0).unwrap_or(15);
    e.tag(Tag::new(TAG_NEGATIVE_BIGNUM))?;
    e.bytes(&bytes[first..])?;
    Ok(())
}

/// Concatenate the chunks of an indefinite-length CBOR byte string into a single `Vec<u8>`,
/// pre-sized so the output buffer is allocated exactly once. Collecting the chunk references
/// up front lets us compute the total length without paying for repeated `extend_from_slice`
/// growth.
fn collect_indef_bytes(d: &mut Decoder<'_>) -> Result<Vec<u8>, decode::Error> {
    let chunks: Vec<&[u8]> = d.bytes_iter()?.collect::<Result<_, _>>()?;
    let total: usize = chunks.iter().map(|c| c.len()).sum();
    let mut buf = Vec::with_capacity(total);
    for chunk in chunks {
        buf.extend_from_slice(chunk);
    }
    Ok(buf)
}

fn read_bignum_payload(d: &mut Decoder<'_>) -> Result<u128, decode::Error> {
    let bytes = d.bytes()?;
    if bytes.len() > 16 {
        return Err(decode::Error::message("bignum payload exceeds 128 bits"));
    }
    let mut buf = [0u8; 16];
    buf[16 - bytes.len()..].copy_from_slice(bytes);
    Ok(u128::from_be_bytes(buf))
}

fn decode_u128(d: &mut Decoder<'_>) -> Result<u128, decode::Error> {
    // Accept a plain CBOR unsigned integer too, since RFC 8949 lets bignums skip the tag when the
    // value fits in u64.
    let ty = d.datatype()?;
    match ty {
        Type::U8 | Type::U16 | Type::U32 | Type::U64 => Ok(u128::from(d.u64()?)),
        Type::Tag => {
            let tag: u64 = d.tag()?.into();
            if tag != TAG_POSITIVE_BIGNUM {
                return Err(decode::Error::message("u128: expected positive-bignum tag (2)"));
            }
            read_bignum_payload(d)
        },
        other => Err(decode::Error::type_mismatch(other).with_message("u128: unexpected CBOR type")),
    }
}

fn decode_i128(d: &mut Decoder<'_>) -> Result<i128, decode::Error> {
    let ty = d.datatype()?;
    match ty {
        Type::U8 | Type::U16 | Type::U32 | Type::U64 => Ok(i128::from(d.u64()?)),
        Type::I8 | Type::I16 | Type::I32 | Type::I64 => Ok(i128::from(d.i64()?)),
        Type::Tag => {
            let tag: u64 = d.tag()?.into();
            let magnitude = read_bignum_payload(d)?;
            match tag {
                TAG_POSITIVE_BIGNUM => i128::try_from(magnitude)
                    .map_err(|_| decode::Error::message("i128: positive bignum overflows i128")),
                TAG_NEGATIVE_BIGNUM => {
                    let signed = i128::try_from(magnitude)
                        .map_err(|_| decode::Error::message("i128: negative bignum overflows i128"))?;
                    Ok(-1i128 - signed)
                },
                _ => Err(decode::Error::message("i128: unexpected bignum tag")),
            }
        },
        other => Err(decode::Error::type_mismatch(other).with_message("i128: unexpected CBOR type")),
    }
}

// ============================================================================
// Serializer
// ============================================================================

#[derive(Debug, Clone)]
pub struct Serializer<W> {
    encoder: Encoder<W>,
    unit_as_null: bool,
}

impl<W: Write> Serializer<W> {
    pub fn new(w: W) -> Self {
        Self::from(Encoder::new(w))
    }

    pub fn encoder_mut(&mut self) -> &mut Encoder<W> {
        &mut self.encoder
    }

    pub fn serialize_unit_as_null(&mut self, enable: bool) -> &mut Self {
        self.unit_as_null = enable;
        self
    }
}

impl<W: Write> From<Encoder<W>> for Serializer<W> {
    fn from(e: Encoder<W>) -> Self {
        Self {
            encoder: e,
            unit_as_null: false,
        }
    }
}

impl<'a, W: Write> ser::Serializer for &'a mut Serializer<W>
where <W as Write>::Error: core::error::Error + 'static
{
    type Error = EncodeError<W::Error>;
    type Ok = ();
    type SerializeMap = SeqSerializer<'a, W>;
    type SerializeSeq = SeqSerializer<'a, W>;
    type SerializeStruct = SeqSerializer<'a, W>;
    type SerializeStructVariant = SeqSerializer<'a, W>;
    type SerializeTuple = SeqSerializer<'a, W>;
    type SerializeTupleStruct = SeqSerializer<'a, W>;
    type SerializeTupleVariant = SeqSerializer<'a, W>;

    fn serialize_bool(self, v: bool) -> Result<(), Self::Error> {
        self.encoder.bool(v)?;
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<(), Self::Error> {
        self.encoder.i8(v)?;
        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<(), Self::Error> {
        self.encoder.i16(v)?;
        Ok(())
    }

    fn serialize_i32(self, v: i32) -> Result<(), Self::Error> {
        self.encoder.i32(v)?;
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<(), Self::Error> {
        self.encoder.i64(v)?;
        Ok(())
    }

    fn serialize_i128(self, v: i128) -> Result<(), Self::Error> {
        encode_i128(&mut self.encoder, v).map_err(Into::into)
    }

    fn serialize_u8(self, v: u8) -> Result<(), Self::Error> {
        self.encoder.u8(v)?;
        Ok(())
    }

    fn serialize_u16(self, v: u16) -> Result<(), Self::Error> {
        self.encoder.u16(v)?;
        Ok(())
    }

    fn serialize_u32(self, v: u32) -> Result<(), Self::Error> {
        self.encoder.u32(v)?;
        Ok(())
    }

    fn serialize_u64(self, v: u64) -> Result<(), Self::Error> {
        self.encoder.u64(v)?;
        Ok(())
    }

    fn serialize_u128(self, v: u128) -> Result<(), Self::Error> {
        encode_u128(&mut self.encoder, v).map_err(Into::into)
    }

    fn serialize_f32(self, v: f32) -> Result<(), Self::Error> {
        self.encoder.f32(v)?;
        Ok(())
    }

    fn serialize_f64(self, v: f64) -> Result<(), Self::Error> {
        self.encoder.f64(v)?;
        Ok(())
    }

    fn serialize_char(self, v: char) -> Result<(), Self::Error> {
        self.encoder.char(v)?;
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Result<(), Self::Error> {
        self.encoder.str(v)?;
        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<(), Self::Error> {
        self.encoder.bytes(v)?;
        Ok(())
    }

    fn serialize_none(self) -> Result<(), Self::Error> {
        self.encoder.null()?;
        Ok(())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), Self::Error> {
        if self.unit_as_null {
            self.encoder.null()?;
        } else {
            self.encoder.encode(())?;
        }
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Self::Error> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<(), Self::Error> {
        variant.serialize(self)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.encoder.map(1)?.str(variant)?;
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        if let Some(n) = len {
            self.encoder.array(n as u64)?;
        } else {
            self.encoder.begin_array()?;
        }
        Ok(SeqSerializer {
            serializer: self,
            indefinite: len.is_none(),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.encoder.array(len as u64)?;
        Ok(SeqSerializer {
            serializer: self,
            indefinite: false,
        })
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.serialize_tuple(len)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.encoder.map(1)?.str(variant)?;
        self.serialize_tuple(len)
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        if let Some(n) = len {
            self.encoder.map(n as u64)?;
        } else {
            self.encoder.begin_map()?;
        }
        Ok(SeqSerializer {
            serializer: self,
            indefinite: len.is_none(),
        })
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct, Self::Error> {
        self.encoder.map(len as u64)?;
        Ok(SeqSerializer {
            serializer: self,
            indefinite: false,
        })
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.encoder.map(1)?.str(variant)?;
        self.serialize_struct(name, len)
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

pub struct SeqSerializer<'a, W: 'a> {
    serializer: &'a mut Serializer<W>,
    indefinite: bool,
}

impl<W: Write> SerializeSeq for SeqSerializer<'_, W>
where <W as Write>::Error: core::error::Error + 'static
{
    type Error = EncodeError<W::Error>;
    type Ok = ();

    fn serialize_element<T: Serialize + ?Sized>(&mut self, x: &T) -> Result<(), Self::Error> {
        x.serialize(&mut *self.serializer)
    }

    fn end(self) -> Result<(), Self::Error> {
        if self.indefinite {
            self.serializer.encoder.end()?;
        }
        Ok(())
    }
}

impl<W: Write> SerializeTuple for SeqSerializer<'_, W>
where <W as Write>::Error: core::error::Error + 'static
{
    type Error = EncodeError<W::Error>;
    type Ok = ();

    fn serialize_element<T: Serialize + ?Sized>(&mut self, x: &T) -> Result<(), Self::Error> {
        x.serialize(&mut *self.serializer)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<W: Write> SerializeTupleStruct for SeqSerializer<'_, W>
where <W as Write>::Error: core::error::Error + 'static
{
    type Error = EncodeError<W::Error>;
    type Ok = ();

    fn serialize_field<T: Serialize + ?Sized>(&mut self, x: &T) -> Result<(), Self::Error> {
        x.serialize(&mut *self.serializer)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<W: Write> SerializeTupleVariant for SeqSerializer<'_, W>
where <W as Write>::Error: core::error::Error + 'static
{
    type Error = EncodeError<W::Error>;
    type Ok = ();

    fn serialize_field<T: Serialize + ?Sized>(&mut self, x: &T) -> Result<(), Self::Error> {
        x.serialize(&mut *self.serializer)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<W: Write> SerializeMap for SeqSerializer<'_, W>
where <W as Write>::Error: core::error::Error + 'static
{
    type Error = EncodeError<W::Error>;
    type Ok = ();

    fn serialize_key<T: Serialize + ?Sized>(&mut self, k: &T) -> Result<(), Self::Error> {
        k.serialize(&mut *self.serializer)
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        v.serialize(&mut *self.serializer)
    }

    fn end(self) -> Result<(), Self::Error> {
        if self.indefinite {
            self.serializer.encoder.end()?;
        }
        Ok(())
    }
}

impl<W: Write> SerializeStruct for SeqSerializer<'_, W>
where <W as Write>::Error: core::error::Error + 'static
{
    type Error = EncodeError<W::Error>;
    type Ok = ();

    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, val: &T) -> Result<(), Self::Error> {
        key.serialize(&mut *self.serializer)?;
        val.serialize(&mut *self.serializer)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<W: Write> SerializeStructVariant for SeqSerializer<'_, W>
where <W as Write>::Error: core::error::Error + 'static
{
    type Error = EncodeError<W::Error>;
    type Ok = ();

    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, val: &T) -> Result<(), Self::Error> {
        key.serialize(&mut *self.serializer)?;
        val.serialize(&mut *self.serializer)
    }

    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

// ============================================================================
// Deserializer
// ============================================================================

#[derive(Debug, Clone)]
pub struct Deserializer<'de> {
    decoder: Decoder<'de>,
}

impl<'de> Deserializer<'de> {
    pub fn new(b: &'de [u8]) -> Self {
        Self::from(Decoder::new(b))
    }

    pub fn decoder_mut(&mut self) -> &mut Decoder<'de> {
        &mut self.decoder
    }

    fn current(&self) -> Result<u8, decode::Error> {
        if let Some(b) = self.decoder.input().get(self.decoder.position()) {
            return Ok(*b);
        }
        Err(decode::Error::end_of_input())
    }

    fn read(&mut self) -> Result<u8, decode::Error> {
        let p = self.decoder.position();
        if let Some(b) = self.decoder.input().get(p) {
            self.decoder.set_position(p + 1);
            return Ok(*b);
        }
        Err(decode::Error::end_of_input())
    }
}

impl<'de> From<Decoder<'de>> for Deserializer<'de> {
    fn from(d: Decoder<'de>) -> Self {
        Self { decoder: d }
    }
}

impl<'de> de::Deserializer<'de> for &mut Deserializer<'de> {
    type Error = DecodeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.decoder.datatype()? {
            Type::Bool => self.deserialize_bool(visitor),
            Type::U8 => self.deserialize_u8(visitor),
            Type::U16 => self.deserialize_u16(visitor),
            Type::U32 => self.deserialize_u32(visitor),
            Type::U64 => self.deserialize_u64(visitor),
            Type::I8 => self.deserialize_i8(visitor),
            Type::I16 => self.deserialize_i16(visitor),
            Type::I32 => self.deserialize_i32(visitor),
            Type::I64 => self.deserialize_i64(visitor),
            Type::F32 => self.deserialize_f32(visitor),
            Type::F64 => self.deserialize_f64(visitor),
            Type::Bytes => visitor.visit_borrowed_bytes(self.decoder.bytes()?),
            Type::String => visitor.visit_borrowed_str(self.decoder.str()?),
            Type::Null => {
                self.decoder.skip()?;
                visitor.visit_none()
            },
            Type::Array | Type::ArrayIndef => self.deserialize_seq(visitor),
            Type::Map | Type::MapIndef => self.deserialize_map(visitor),
            Type::Tag => {
                // Could be a bignum (positive or negative). decode_i128 handles both
                // TAG_POSITIVE_BIGNUM and TAG_NEGATIVE_BIGNUM; anything else is currently
                // unsupported, matching upstream.
                let v = decode_i128(&mut self.decoder)?;
                visitor.visit_i128(v)
            },
            Type::BytesIndef => visitor.visit_byte_buf(collect_indef_bytes(&mut self.decoder)?),
            Type::StringIndef => {
                #[cfg(feature = "std")]
                let mut buf = std::string::String::new();
                #[cfg(not(feature = "std"))]
                let mut buf = alloc::string::String::new();
                for b in self.decoder.str_iter()? {
                    buf += b?;
                }
                visitor.visit_string(buf)
            },
            t @ (Type::F16 | Type::Undefined | Type::Int | Type::Simple | Type::Break | Type::Unknown(_)) => {
                Err(decode::Error::type_mismatch(t)
                    .with_message("unexpected type")
                    .at(self.decoder.position())
                    .into())
            },
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_bool(self.decoder.bool()?)
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_i8(self.decoder.i8()?)
    }

    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_i16(self.decoder.i16()?)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_i32(self.decoder.i32()?)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_i64(self.decoder.i64()?)
    }

    fn deserialize_i128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let v = decode_i128(&mut self.decoder)?;
        visitor.visit_i128(v)
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u8(self.decoder.u8()?)
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u16(self.decoder.u16()?)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u32(self.decoder.u32()?)
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u64(self.decoder.u64()?)
    }

    fn deserialize_u128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let v = decode_u128(&mut self.decoder)?;
        visitor.visit_u128(v)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_f32(self.decoder.f32()?)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_f64(self.decoder.f64()?)
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_char(self.decoder.char()?)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_borrowed_str(self.decoder.str()?)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_str(self.decoder.str()?)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_borrowed_bytes(self.decoder.bytes()?)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_bytes(self.decoder.bytes()?)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if Type::Null == self.decoder.datatype()? {
            self.decoder.skip()?;
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.decoder.decode::<()>()?;
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(self, _name: &'static str, v: V) -> Result<V::Value, Self::Error> {
        self.deserialize_unit(v)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _name: &'static str, v: V) -> Result<V::Value, Self::Error> {
        v.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        // serde's default `Deserialize for Vec<u8>` (and therefore `Cow<'_, [u8]>`, which goes
        // through `Vec<u8>`) calls `deserialize_seq` even though the wire form is a CBOR byte
        // string. Its visitor only handles `visit_seq`. So when the encoded form is a CBOR byte
        // string, surface each byte as a one-element seq item. Without this, any foreign type
        // using `#[serde(with = "hex_or_bytes")]` (e.g. `tari_sidechain::SidechainBlockHeader`)
        // fails to round-trip.
        match self.decoder.datatype()? {
            Type::Bytes => {
                let bytes = self.decoder.bytes()?;
                visitor.visit_seq(ByteSeqAccess::new(bytes))
            },
            Type::BytesIndef => {
                let buf = collect_indef_bytes(&mut self.decoder)?;
                visitor.visit_seq(ByteSeqAccessOwned::new(buf))
            },
            _ => {
                let len = self.decoder.array()?;
                visitor.visit_seq(Seq::new(self, len))
            },
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error> {
        let p = self.decoder.position();
        let n = self.decoder.array()?;
        if Some(len as u64) != n {
            return Err(
                decode::Error::message(format!("invalid length {n:?}, was expecting: {len}"))
                    .at(p)
                    .into(),
            );
        }
        visitor.visit_seq(Seq::new(self, n))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let len = self.decoder.map()?;
        visitor.visit_map(Seq::new(self, len))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let p = self.decoder.position();
        if Type::Map == self.decoder.datatype()? {
            let m = self.decoder.map()?;
            if m != Some(1) {
                return Err(decode::Error::message("invalid enum map length").at(p).into());
            }
        }
        visitor.visit_enum(Enum::new(self))
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.decoder.skip()?;
        visitor.visit_unit()
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

struct Seq<'a, 'de> {
    deserializer: &'a mut Deserializer<'de>,
    len: Option<u64>,
}

impl<'a, 'de> Seq<'a, 'de> {
    fn new(d: &'a mut Deserializer<'de>, len: Option<u64>) -> Self {
        Self { deserializer: d, len }
    }
}

impl<'de> SeqAccess<'de> for Seq<'_, 'de> {
    type Error = DecodeError;

    fn size_hint(&self) -> Option<usize> {
        self.len.and_then(|n| n.try_into().ok())
    }

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error> {
        match self.len {
            None => {
                if BREAK == self.deserializer.current()? {
                    self.deserializer.read()?;
                    Ok(None)
                } else {
                    seed.deserialize(&mut *self.deserializer).map(Some)
                }
            },
            Some(0) => Ok(None),
            Some(n) => {
                let x = seed.deserialize(&mut *self.deserializer)?;
                self.len = Some(n - 1);
                Ok(Some(x))
            },
        }
    }
}

impl<'de> MapAccess<'de> for Seq<'_, 'de> {
    type Error = DecodeError;

    fn size_hint(&self) -> Option<usize> {
        self.len.and_then(|n| n.try_into().ok())
    }

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        match self.len {
            None => {
                if BREAK == self.deserializer.current()? {
                    self.deserializer.read()?;
                    Ok(None)
                } else {
                    seed.deserialize(&mut *self.deserializer).map(Some)
                }
            },
            Some(0) => Ok(None),
            Some(_) => seed.deserialize(&mut *self.deserializer).map(Some),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        if let Some(n) = self.len {
            let x = seed.deserialize(&mut *self.deserializer)?;
            self.len = Some(n - 1);
            Ok(x)
        } else {
            seed.deserialize(&mut *self.deserializer)
        }
    }
}

/// `SeqAccess` that yields the bytes of a borrowed slice one at a time, deserializing each as
/// a `u8`. Used by [`Deserializer::deserialize_seq`] to satisfy `Vec<u8>` / `Cow<'_, [u8]>`
/// visitors when the underlying CBOR form is a byte string rather than an array.
struct ByteSeqAccess<'de> {
    bytes: &'de [u8],
    pos: usize,
}

impl<'de> ByteSeqAccess<'de> {
    fn new(bytes: &'de [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
}

impl<'de> SeqAccess<'de> for ByteSeqAccess<'de> {
    type Error = DecodeError;

    fn size_hint(&self) -> Option<usize> {
        Some(self.bytes.len() - self.pos)
    }

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error> {
        if self.pos >= self.bytes.len() {
            return Ok(None);
        }
        let byte = self.bytes[self.pos];
        self.pos += 1;
        seed.deserialize(serde::de::value::U8Deserializer::new(byte)).map(Some)
    }
}

/// Owned variant of [`ByteSeqAccess`] for indefinite-length CBOR byte strings, where the
/// individual chunks have to be concatenated into a heap buffer.
struct ByteSeqAccessOwned {
    bytes: Vec<u8>,
    pos: usize,
}

impl ByteSeqAccessOwned {
    fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, pos: 0 }
    }
}

impl<'de> SeqAccess<'de> for ByteSeqAccessOwned {
    type Error = DecodeError;

    fn size_hint(&self) -> Option<usize> {
        Some(self.bytes.len() - self.pos)
    }

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error> {
        if self.pos >= self.bytes.len() {
            return Ok(None);
        }
        let byte = self.bytes[self.pos];
        self.pos += 1;
        seed.deserialize(serde::de::value::U8Deserializer::new(byte)).map(Some)
    }
}

struct Enum<'a, 'de: 'a> {
    deserializer: &'a mut Deserializer<'de>,
}

impl<'a, 'de> Enum<'a, 'de> {
    fn new(d: &'a mut Deserializer<'de>) -> Self {
        Self { deserializer: d }
    }
}

impl<'de> EnumAccess<'de> for Enum<'_, 'de> {
    type Error = DecodeError;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error> {
        seed.deserialize(&mut *self.deserializer).map(|v| (v, self))
    }
}

impl<'de> VariantAccess<'de> for Enum<'_, 'de> {
    type Error = DecodeError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Self::Error> {
        seed.deserialize(self.deserializer)
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, v: V) -> Result<V::Value, Self::Error> {
        de::Deserializer::deserialize_tuple(self.deserializer, len, v)
    }

    fn struct_variant<V: Visitor<'de>>(self, _fields: &'static [&'static str], v: V) -> Result<V::Value, Self::Error> {
        de::Deserializer::deserialize_map(self.deserializer, v)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    fn roundtrip<T>(v: &T) -> T
    where T: Serialize + serde::de::DeserializeOwned + PartialEq + core::fmt::Debug {
        let bytes = to_vec(v).unwrap();
        let got: T = from_slice(&bytes).unwrap();
        assert_eq!(&got, v);
        got
    }

    #[test]
    fn u128_boundary_values() {
        roundtrip(&0u128);
        roundtrip(&1u128);
        roundtrip(&u128::from(u64::MAX));
        roundtrip(&(u128::from(u64::MAX) + 1));
        roundtrip(&u128::MAX);
    }

    #[test]
    fn i128_boundary_values() {
        roundtrip(&0i128);
        roundtrip(&1i128);
        roundtrip(&-1i128);
        roundtrip(&i128::from(i64::MAX));
        roundtrip(&(i128::from(i64::MAX) + 1));
        roundtrip(&i128::from(i64::MIN));
        roundtrip(&(i128::from(i64::MIN) - 1));
        roundtrip(&i128::MAX);
        roundtrip(&i128::MIN);
    }

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Outer {
        name: String,
        big: u128,
        sized: i64,
    }

    #[test]
    fn nested_u128_in_struct() {
        let v = Outer {
            name: "tx-receipt".into(),
            big: u128::from(u64::MAX) + 1,
            sized: -42,
        };
        roundtrip(&v);
    }

    /// Mirrors the `#[serde(with = "hex_or_bytes")]` shape used by foreign types in
    /// `tari_sidechain`: encode borrows of byte slices as CBOR `bstr`, then deserialize back via
    /// `Cow<'_, [u8]>` / `Vec<u8>`. Catches the regression where `deserialize_seq` was called by
    /// the inner visitor but the encoded form was a CBOR byte string.
    mod bytes_compat {
        use std::borrow::Cow;

        use serde::{Deserialize, Serialize, Serializer};

        mod bytes_field {
            use super::*;

            pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &T, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_bytes(v.as_ref())
            }

            pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
            where
                D: serde::Deserializer<'de>,
                T: for<'a> TryFrom<&'a [u8]>,
                for<'a> <T as TryFrom<&'a [u8]>>::Error: std::fmt::Display,
            {
                let bytes = <Cow<'_, [u8]> as Deserialize>::deserialize(d)?;
                T::try_from(bytes.as_ref()).map_err(serde::de::Error::custom)
            }
        }

        #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
        struct WithBytes {
            #[serde(with = "bytes_field")]
            hash: [u8; 32],
            tail: u32,
        }

        #[test]
        fn cow_bytes_via_serialize_bytes() {
            let v = WithBytes {
                hash: [7; 32],
                tail: 0xdead_beef,
            };
            super::roundtrip(&v);
        }
    }
}
