//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::any::type_name;

use anyhow::anyhow;
use tari_common_types::types::FixedHash;
use tari_dan_storage::consensus_models::BlockId;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
    utils::read_to_fixed,
};

/// NOTE: you cannot use this codec with prefixes unless T::try_from(bytes) does not error with excess bytes.
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

    fn decode(&self, bytes: &[u8]) -> Result<T, RocksDbStorageError> {
        let ret = T::try_from(bytes).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!("{}: {}", type_name::<T>(), e),
        })?;
        Ok(ret)
    }
}
pub type FixedBytesCodec32 = FixedBytesCodec<32>;
pub type TransactionIdCodec = FixedBytesCodec<32>;

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

    fn decode(&self, mut bytes: &[u8]) -> Result<T, RocksDbStorageError> {
        let fixed = read_to_fixed(&mut bytes).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("FixedBytesCodec: Expected {} bytes, got {}", LEN, bytes.len()),
        })?;
        let ret = T::from(fixed);
        Ok(ret)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BlockIdCodec;

impl DbCodec<BlockId> for BlockIdCodec {
    fn encode(&self, value: &BlockId) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(value.into_array()))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<BlockId, RocksDbStorageError> {
        let hash = read_to_fixed(&mut bytes).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("BlockIdCodec: failed to decode too few bytes"),
        })?;
        Ok(BlockId::new(FixedHash::new(hash)))
    }
}
