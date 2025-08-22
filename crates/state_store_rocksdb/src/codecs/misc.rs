//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Read;

use tari_ootle_common_types::{shard::Shard, Epoch, NodeHeight};

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
    utils::read_to_fixed,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct UnitCodec;

impl DbCodec<()> for UnitCodec {
    fn encode(&self, _: &()) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::empty())
    }

    fn decode_reader<R: Read>(&self, _reader: &mut R) -> Result<(), RocksDbStorageError> {
        Ok(())
    }
}

#[derive(Default)]
pub struct BoolCodec;

impl DbCodec<bool> for BoolCodec {
    fn encode(&self, value: &bool) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array([u8::from(*value)]))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<bool, RocksDbStorageError> {
        let mut buf = [0u8; 1];
        reader
            .read_exact(&mut buf)
            .map_err(|e| RocksDbStorageError::MalformedData {
                operation: "decode u8",
                details: format!("Failed to read 1 byte for NumberCodec<u8>: {e}"),
            })?;
        Ok(buf[0] != 0)
    }
}

pub type EpochCodec = NumberCodec<Epoch>;
pub type ShardCodec = NumberCodec<Shard>;
pub type NodeHeightCodec = NumberCodec<NodeHeight>;

#[derive(Debug, Clone, Copy)]
pub struct NumberCodec<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl DbCodec<u8> for NumberCodec<u8> {
    fn encode(&self, value: &u8) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array([*value]))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<u8, RocksDbStorageError> {
        let mut buf = [0u8; 1];
        reader
            .read_exact(&mut buf)
            .map_err(|e| RocksDbStorageError::MalformedData {
                operation: "decode u8",
                details: format!("Failed to read 1 byte for NumberCodec<u8>: {e}"),
            })?;
        Ok(buf[0])
    }
}

impl DbCodec<u32> for NumberCodec<u32> {
    fn encode(&self, value: &u32) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(value.to_be_bytes()))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<u32, RocksDbStorageError> {
        let arr = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode u32",
            details: "Invalid bytes size for NumberCodec<u32>".to_string(),
        })?;
        Ok(u32::from_be_bytes(arr))
    }
}

impl DbCodec<u64> for NumberCodec<u64> {
    fn encode(&self, value: &u64) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(value.to_be_bytes()))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<u64, RocksDbStorageError> {
        let arr = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode u64",
            details: "Invalid bytes size for NumberCodec<u64>".to_string(),
        })?;
        Ok(u64::from_be_bytes(arr))
    }
}

impl DbCodec<Epoch> for NumberCodec<Epoch> {
    fn encode(&self, epoch: &Epoch) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(epoch.to_be_bytes()))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<Epoch, RocksDbStorageError> {
        let arr = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode Epoch",
            details: "Invalid bytes size for NumberCodec<Epoch>".to_string(),
        })?;
        let epoch = u64::from_be_bytes(arr);
        Ok(Epoch(epoch))
    }
}

impl DbCodec<NodeHeight> for NumberCodec<NodeHeight> {
    fn encode(&self, value: &NodeHeight) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(value.as_u64().to_be_bytes()))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<NodeHeight, RocksDbStorageError> {
        let arr = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode NodeHeight",
            details: "Invalid bytes size for NumberCodec<NodeHeight>".to_string(),
        })?;
        let height = u64::from_be_bytes(arr);
        Ok(NodeHeight::from(height))
    }
}

impl DbCodec<Shard> for NumberCodec<Shard> {
    fn encode(&self, shard: &Shard) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(shard.as_u32().to_be_bytes()))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<Shard, RocksDbStorageError> {
        let arr = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode Shard",
            details: "Invalid bytes size for NumberCodec<Shard>".to_string(),
        })?;
        let shard = u32::from_be_bytes(arr);
        Ok(shard.into())
    }
}

impl<T> Default for NumberCodec<T> {
    fn default() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}
