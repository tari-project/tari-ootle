//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::{shard::Shard, Epoch, NodeHeight};

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

    fn decode(&self, _: &[u8]) -> Result<(), RocksDbStorageError> {
        Ok(())
    }
}

#[derive(Default)]
pub struct BoolCodec;

impl DbCodec<bool> for BoolCodec {
    fn encode(&self, value: &bool) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array([u8::from(*value)]))
    }

    fn decode(&self, bytes: &[u8]) -> Result<bool, RocksDbStorageError> {
        if bytes.is_empty() {
            return Err(RocksDbStorageError::MalformedData {
                operation: "decode bool",
                details: "Empty bytes".to_string(),
            });
        }
        Ok(bytes[0] != 0)
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

    fn decode(&self, bytes: &[u8]) -> Result<u8, RocksDbStorageError> {
        if bytes.is_empty() {
            return Err(RocksDbStorageError::MalformedData {
                operation: "decode u8",
                details: "Empty bytes".to_string(),
            });
        }
        Ok(bytes[0])
    }
}

impl DbCodec<u32> for NumberCodec<u32> {
    fn encode(&self, value: &u32) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(value.to_be_bytes()))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<u32, RocksDbStorageError> {
        let buf = read_to_fixed(&mut bytes).ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode u32",
            details: format!("Invalid bytes size {} for NumberCodec", bytes.len()),
        })?;
        Ok(u32::from_be_bytes(buf))
    }
}

impl DbCodec<u64> for NumberCodec<u64> {
    fn encode(&self, value: &u64) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(value.to_be_bytes()))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<u64, RocksDbStorageError> {
        let buf = read_to_fixed(&mut bytes).ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode u64",
            details: format!("Invalid bytes size {} for NumberCodec", bytes.len()),
        })?;
        Ok(u64::from_be_bytes(buf))
    }
}

impl DbCodec<Epoch> for NumberCodec<Epoch> {
    fn encode(&self, epoch: &Epoch) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(epoch.to_be_bytes()))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<Epoch, RocksDbStorageError> {
        let arr = read_to_fixed(&mut bytes).ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode Epoch",
            details: format!("Invalid bytes size {} for EpochCodec", bytes.len()),
        })?;
        let epoch = u64::from_be_bytes(arr);
        Ok(Epoch(epoch))
    }
}

impl DbCodec<NodeHeight> for NumberCodec<NodeHeight> {
    fn encode(&self, value: &NodeHeight) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(value.as_u64().to_be_bytes()))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<NodeHeight, RocksDbStorageError> {
        let buf = read_to_fixed(&mut bytes).ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode NodeHeight",
            details: format!("Invalid bytes size {} for NumberCodec", bytes.len()),
        })?;
        Ok(NodeHeight::from(u64::from_be_bytes(buf)))
    }
}

impl DbCodec<Shard> for NumberCodec<Shard> {
    fn encode(&self, shard: &Shard) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array(shard.as_u32().to_be_bytes()))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<Shard, RocksDbStorageError> {
        let arr = read_to_fixed(&mut bytes).ok_or_else(|| RocksDbStorageError::MalformedData {
            operation: "decode Shard",
            details: format!("Invalid bytes size {} for ShardCodec", bytes.len()),
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
