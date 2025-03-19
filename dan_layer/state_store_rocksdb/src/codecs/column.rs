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
/// Great care should be taken when using this on a shared column family that uses prefix lookups.
/// for e.g. a <blockid> key and a <column><block_id> key can share the same shorter prefix leading to bugs that are
/// difficult to debug.
#[derive(Debug)]
pub struct Column<const COL: u32>;

impl<const COL: u32> Column<COL> {
    pub const fn new() -> Self {
        Self
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
        Ok(Column::new())
    }
}

impl<const COL: u32> Display for Column<COL> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Column<{COL}>")
    }
}
