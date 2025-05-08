//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::time::{OffsetDateTime, PrimitiveDateTime};

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
    utils::read_to_fixed,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct DateTimeCodec;

impl DbCodec<PrimitiveDateTime> for DateTimeCodec {
    fn encode(&self, value: &PrimitiveDateTime) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(
            value.assume_utc().unix_timestamp().to_be_bytes(),
        ))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<PrimitiveDateTime, RocksDbStorageError> {
        let buf = read_to_fixed(&mut bytes).ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode u32",
            details: format!("Invalid bytes size {} for NumberCodec", bytes.len()),
        })?;
        let timestamp = i64::from_be_bytes(buf);
        let offset =
            OffsetDateTime::from_unix_timestamp(timestamp).map_err(|e| RocksDbStorageError::MalformedData {
                operation: "decode PrimitiveDateTime",
                details: format!("Invalid timestamp {}: {}", timestamp, e),
            })?;
        Ok(PrimitiveDateTime::new(offset.date(), offset.time()))
    }
}
