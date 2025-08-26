//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display, io::Read};

use anyhow::anyhow;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
    utils::read_to_fixed,
};

/// A const key used to differentiate "columns" in a reused column family.
/// This hard codes 32 bytes (big-endian) from the encoded bytes.
/// It is not recommended to use this on a shared column family that uses prefix lookups, as the codec used would need
/// to account for keys that have the column bytes encoded in it. For e.g. a <foo> key and a <column><foo> key
/// can share the same shorter prefix leading to bugs that are difficult to debug.
#[derive(Debug)]
pub struct Column<const COL: u32>;

impl<const COL: u32> Display for Column<COL> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Column<{COL}>")
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ByteColumn<const COL: u8>;
impl<const COL: u8> ByteColumn<COL> {
    pub const fn byte(&self) -> u8 {
        COL
    }
}
impl<const COL: u8> Display for ByteColumn<COL> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ByteColumn<{COL}>")
    }
}

#[derive(Default)]
pub struct ColumnCodec;

impl<const COL: u32> DbCodec<Column<COL>> for ColumnCodec {
    fn encode(&self, _value: &Column<COL>) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(COL.to_be_bytes()))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<Column<COL>, RocksDbStorageError> {
        let arr = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("Invalid bytes for ColumnCodec"),
        })?;

        let col = u32::from_be_bytes(arr);
        if col != COL {
            return Err(RocksDbStorageError::DecodeError {
                source: anyhow!(
                    "Invalid column '{}', ColumnCodec expected big-endian bytes for '{}'",
                    col,
                    COL
                ),
            });
        }
        Ok(Column)
    }
}

impl<const COL: u8> DbCodec<ByteColumn<COL>> for ColumnCodec {
    fn encode(&self, _value: &ByteColumn<COL>) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array([COL]))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<ByteColumn<COL>, RocksDbStorageError> {
        let mut buf = [0u8; 1];
        reader
            .read_exact(&mut buf)
            .map_err(|e| RocksDbStorageError::MalformedData {
                operation: "decode ByteColumn",
                details: format!("Failed to read 1 byte for ColumnCodec: {e}"),
            })?;

        if buf[0] != COL {
            return Err(RocksDbStorageError::DecodeError {
                source: anyhow!(
                    "Invalid byte column bytes '{}', ColumnCodec expected byte '{}'",
                    buf[0],
                    COL
                ),
            });
        }
        Ok(ByteColumn)
    }
}
