//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
    utils::{read_to_fixed, RocksDbTimestamp},
};

#[derive(Default)]
pub struct TimestampCodec;

impl DbCodec<RocksDbTimestamp> for TimestampCodec {
    fn encode(&self, value: &RocksDbTimestamp) -> Result<EncodeVec, RocksDbStorageError> {
        let buf = value.as_millis().to_be_bytes();
        Ok(EncodeVec::from_slice(&buf))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<RocksDbTimestamp, RocksDbStorageError> {
        let bytes = read_to_fixed(&mut bytes).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("RocksDbTimestampCodec: failed to decode"),
        })?;
        let timestamp = u128::from_be_bytes(bytes);
        Ok(RocksDbTimestamp::from_millis(timestamp))
    }
}
