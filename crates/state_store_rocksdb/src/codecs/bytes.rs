//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{any::type_name, io::Read};

use anyhow::anyhow;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
    utils::read_to_fixed,
};

/// NOTE: you cannot use this codec with prefixes since it uses the entire reader to decode the bytes.
/// Use FixedBytesCodec for this case.
#[derive(Default)]
pub struct BytesCodec;

impl<T> DbCodec<T> for BytesCodec
where
    T: AsRef<[u8]>,
    for<'a> T: TryFrom<&'a [u8]>,
    for<'a> <T as TryFrom<&'a [u8]>>::Error: std::error::Error,
{
    fn encode(&self, value: &T) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::from_slice(value.as_ref()))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<T, RocksDbStorageError> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|e| RocksDbStorageError::MalformedData {
                operation: "decode bytes",
                details: format!("Failed to read bytes for BytesCodec: {e}"),
            })?;
        let ret = T::try_from(bytes.as_slice()).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!("{}: {}", type_name::<T>(), e),
        })?;
        Ok(ret)
    }
}
pub type FixedBytesCodec32 = FixedBytesCodec<32>;
pub type TransactionIdCodec = FixedBytesCodec<32>;
pub type BlockIdCodec = FixedBytesCodec<32>;

#[derive(Debug, Clone, Copy, Default)]
pub struct FixedBytesCodec<const LEN: usize>;

impl<T, const LEN: usize> DbCodec<T> for FixedBytesCodec<LEN>
where
    T: AsRef<[u8]>,
    T: From<[u8; LEN]>,
{
    fn encode(&self, value: &T) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::from_slice(value.as_ref()))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<T, RocksDbStorageError> {
        let fixed = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("FixedBytesCodec: Expected {} bytes", LEN),
        })?;
        let ret = T::from(fixed);
        Ok(ret)
    }
}
