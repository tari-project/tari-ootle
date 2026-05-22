//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display, io::Write};

use anyhow::anyhow;

use crate::{
    codecs::{DbDecoder, DbEncoder, EncodeVec},
    error::RocksDbStorageError,
    utils::take_fixed,
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

impl<const COL: u32> DbEncoder<Column<COL>> for ColumnCodec {
    fn encode_len(&self, _value: &Column<COL>) -> Result<usize, RocksDbStorageError> {
        Ok(size_of::<u32>())
    }

    fn encode_into<W: Write>(&self, _value: &Column<COL>, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(&COL.to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("ColumnCodec: Failed to write column bytes: {}", e),
            })?;
        Ok(())
    }

    fn encode(&self, _value: &Column<COL>) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(COL.to_be_bytes()))
    }
}

impl<const COL: u32> DbDecoder<Column<COL>> for ColumnCodec {
    fn decode(&self, bytes: &[u8]) -> Result<(Column<COL>, usize), RocksDbStorageError> {
        let arr: [u8; 4] = take_fixed(bytes).ok_or_else(|| RocksDbStorageError::DecodeError {
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
        Ok((Column, 4))
    }
}

impl<const COL: u8> DbEncoder<ByteColumn<COL>> for ColumnCodec {
    fn encode_len(&self, _value: &ByteColumn<COL>) -> Result<usize, RocksDbStorageError> {
        Ok(1)
    }

    fn encode_into<W: Write>(&self, _value: &ByteColumn<COL>, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer.write_all(&[COL]).map_err(|e| RocksDbStorageError::EncodeError {
            source: anyhow!("ByteColumnCodec: Failed to write column byte: {}", e),
        })?;
        Ok(())
    }

    fn encode(&self, _value: &ByteColumn<COL>) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array([COL]))
    }
}

impl<const COL: u8> DbDecoder<ByteColumn<COL>> for ColumnCodec {
    fn decode(&self, bytes: &[u8]) -> Result<(ByteColumn<COL>, usize), RocksDbStorageError> {
        let byte = bytes.first().ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode ByteColumn",
            details: "Not enough bytes for ColumnCodec".to_string(),
        })?;

        if *byte != COL {
            return Err(RocksDbStorageError::DecodeError {
                source: anyhow!(
                    "Invalid byte column bytes '{}', ColumnCodec expected byte '{}'",
                    byte,
                    COL
                ),
            });
        }
        Ok((ByteColumn, 1))
    }
}
