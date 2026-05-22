//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{any::type_name, io::Write};

use anyhow::anyhow;

use crate::{
    codecs::{DbDecoder, DbEncoder, EncodeVec},
    error::RocksDbStorageError,
    utils::take_fixed,
};

/// NOTE: you cannot use this codec with prefixes since it uses the entire reader to decode the bytes.
/// Use FixedBytesCodec for this case.
#[derive(Default)]
pub struct BytesCodec;

impl<T: AsRef<[u8]>> DbEncoder<T> for BytesCodec {
    fn encode_len(&self, value: &T) -> Result<usize, RocksDbStorageError> {
        Ok(value.as_ref().len())
    }

    fn encode_into<W: Write>(&self, value: &T, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(value.as_ref())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("BytesCodec: Failed to write bytes: {}", e),
            })?;
        Ok(())
    }

    fn encode(&self, value: &T) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::from_slice(value.as_ref()))
    }
}

impl<T> DbDecoder<T> for BytesCodec
where
    for<'a> T: TryFrom<&'a [u8]>,
    for<'a> <T as TryFrom<&'a [u8]>>::Error: std::error::Error,
{
    fn decode(&self, bytes: &[u8]) -> Result<(T, usize), RocksDbStorageError> {
        let ret = T::try_from(bytes).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!("{}: {}", type_name::<T>(), e),
        })?;
        Ok((ret, bytes.len()))
    }
}
pub type FixedBytesCodec32 = FixedBytesCodec<32>;
pub type TransactionIdCodec = FixedBytesCodec<32>;
pub type BlockIdCodec = FixedBytesCodec<32>;

#[derive(Debug, Clone, Copy, Default)]
pub struct FixedBytesCodec<const LEN: usize>;

impl<T: AsRef<[u8]>, const LEN: usize> DbEncoder<T> for FixedBytesCodec<LEN> {
    fn encode_len(&self, _value: &T) -> Result<usize, RocksDbStorageError> {
        Ok(LEN)
    }

    fn encode_into<W: Write>(&self, value: &T, writer: &mut W) -> Result<(), RocksDbStorageError> {
        let bytes = value.as_ref();
        writer.write_all(bytes).map_err(|e| RocksDbStorageError::EncodeError {
            source: anyhow!("FixedBytesCodec: Failed to write bytes: {}", e),
        })?;
        Ok(())
    }

    fn encode(&self, value: &T) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::from_slice(value.as_ref()))
    }
}

impl<T, const LEN: usize> DbDecoder<T> for FixedBytesCodec<LEN>
where T: From<[u8; LEN]>
{
    fn decode(&self, bytes: &[u8]) -> Result<(T, usize), RocksDbStorageError> {
        let fixed: [u8; LEN] = take_fixed(bytes).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("FixedBytesCodec: Expected {} bytes, got {}", LEN, bytes.len()),
        })?;
        Ok((T::from(fixed), LEN))
    }
}
