//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display};

use anyhow::anyhow;
use tari_dan_common_types::displayable::Displayable;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
};

const SZ_OF_U32: usize = size_of::<u32>();

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

    fn decode(&self, bytes: &[u8]) -> Result<Column<COL>, RocksDbStorageError> {
        if bytes.len() < SZ_OF_U32 {
            return Err(RocksDbStorageError::DecodeError {
                source: anyhow!("Invalid bytes len={} for ColumnCodec", bytes.len()),
            });
        }
        if bytes[..SZ_OF_U32] != COL.to_be_bytes() {
            return Err(RocksDbStorageError::DecodeError {
                source: anyhow!(
                    "Invalid column bytes '{}', ColumnCodec expected big-endian bytes for '{}'",
                    bytes[..SZ_OF_U32].display(),
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

    fn decode(&self, bytes: &[u8]) -> Result<ByteColumn<COL>, RocksDbStorageError> {
        if bytes.is_empty() {
            return Err(RocksDbStorageError::DecodeError {
                source: anyhow!("Invalid bytes len={} for ColumnCodec", bytes.len()),
            });
        }
        if bytes[0] != COL {
            return Err(RocksDbStorageError::DecodeError {
                source: anyhow!(
                    "Invalid byte column bytes '{}', ColumnCodec expected byte '{}'",
                    bytes[0],
                    COL
                ),
            });
        }
        Ok(ByteColumn)
    }
}
