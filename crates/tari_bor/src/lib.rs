//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
use alloc::{fmt, format, string::ToString, vec::Vec};

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(feature = "std")]
use std::fmt;

mod tag;
pub use tag::*;

mod byte_counter;
mod error;
#[cfg(all(feature = "std", feature = "json_encoding"))]
pub mod json_encoding;
mod walker;

pub use ciborium::{cbor, value::Value};
use ciborium::{de::from_reader, ser::into_writer};
pub use ciborium_io::{Read, Write};
pub use error::BorError;
pub use serde::{self, de::DeserializeOwned, Deserialize, Serialize};
pub use walker::*;

pub use crate::byte_counter::ByteCounter;

pub fn encode_with_len<T: Serialize>(val: &T) -> Vec<u8> {
    let mut buf = Vec::with_capacity(512);
    encode_with_len_to_writer(&mut buf, val).expect("Vec<u8> Write impl is infallible");
    buf
}

#[cfg(feature = "std")]
pub fn encode_with_len_to_writer<T, W>(writer: &mut W, val: &T) -> Result<(), BorError>
where
    T: Serialize,
    W: std::io::Write,
{
    let len = encoded_len(val)?;
    writer
        .write_all(&(len as u32).to_le_bytes())
        .map_err(|e| BorError::new(format!("{e:?}")))?;
    encode_into_writer(val, writer)?;
    Ok(())
}

#[cfg(not(feature = "std"))]
pub fn encode_with_len_to_writer<T, W>(writer: &mut W, val: &T) -> Result<(), BorError>
where
    T: Serialize,
    W: Write,
    W::Error: fmt::Debug,
{
    let len = encoded_len(val)?;
    writer
        .write_all(&(len as u32).to_le_bytes())
        .map_err(|e| BorError::new(format!("{e:?}")))?;
    encode_into_writer(val, writer)?;
    Ok(())
}

#[cfg(feature = "std")]
pub fn encode_into_writer<T, W>(val: &T, writer: &mut W) -> Result<(), BorError>
where
    T: Serialize + ?Sized,
    W: std::io::Write,
{
    into_writer(&val, writer).map_err(to_bor_error)
}

#[cfg(not(feature = "std"))]
pub fn encode_into_writer<T, W>(val: &T, writer: &mut W) -> Result<(), BorError>
where
    T: Serialize + ?Sized,
    W: Write,
    W::Error: fmt::Debug,
{
    into_writer(&val, writer).map_err(to_bor_error)
}

pub fn encode<T: Serialize + ?Sized>(val: &T) -> Result<Vec<u8>, BorError> {
    let len = encoded_len(val)?;
    let mut buf = Vec::with_capacity(len);
    encode_into_writer(val, &mut buf)?;
    Ok(buf)
}

pub fn encoded_len<T: Serialize + ?Sized>(val: &T) -> Result<usize, BorError> {
    let mut counter = ByteCounter::new();
    encode_into_writer(val, &mut counter)?;
    Ok(counter.get())
}
pub fn encoded_len_with_limit<T: Serialize + ?Sized>(val: &T, limit: usize) -> Result<usize, BorError> {
    let mut counter = ByteCounter::with_limit(limit);
    encode_into_writer(val, &mut counter)?;
    Ok(counter.get())
}

/// Encodes any Rust type using CBOR
pub fn to_value<T: Serialize + ?Sized>(val: &T) -> Result<Value, BorError> {
    Value::serialized(val).map_err(to_bor_error)
}

pub fn from_value<T: DeserializeOwned>(val: &Value) -> Result<T, BorError> {
    Value::deserialized(val).map_err(to_bor_error)
}

pub fn decode<T: DeserializeOwned>(mut input: &[u8]) -> Result<T, BorError> {
    decode_inner(&mut input)
}

fn decode_inner<T: DeserializeOwned>(input: &mut &[u8]) -> Result<T, BorError> {
    let result = from_reader(input).map_err(to_bor_error)?;
    Ok(result)
}

pub fn decode_from_reader<T, R>(reader: R) -> Result<T, BorError>
where
    T: DeserializeOwned,
    R: Read,
    R::Error: fmt::Debug,
{
    let result = from_reader(reader).map_err(to_bor_error)?;
    Ok(result)
}

pub fn decode_exact<T: DeserializeOwned>(mut input: &[u8]) -> Result<T, BorError> {
    let val = decode_inner(&mut input)?;
    if !input.is_empty() {
        return Err(BorError::new(format!(
            "decode_exact: {} bytes remaining on input",
            input.len()
        )));
    }
    Ok(val)
}

pub fn decode_len(input: &[u8]) -> Result<usize, BorError> {
    if input.len() < 4 {
        return Err(BorError::new("Not enough bytes to decode length".to_string()));
    }

    let mut buf = [0u8; 4];
    buf.copy_from_slice(input.get(..4).unwrap());
    let len = u32::from_le_bytes(buf);
    Ok(len as usize)
}

fn to_bor_error<E>(e: E) -> BorError
where E: fmt::Display {
    BorError::new(e.to_string())
}
