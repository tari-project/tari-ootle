//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::{Read, Write};

use tari_ootle_common_types::{Epoch, NodeHeight, shard::Shard};

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
    utils::read_to_fixed,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct UnitCodec;

impl DbCodec<()> for UnitCodec {
    fn encode_len(&self, _value: &()) -> Result<usize, RocksDbStorageError> {
        Ok(0)
    }

    fn encode_into<W: Write>(&self, _value: &(), _writer: &mut W) -> Result<(), RocksDbStorageError> {
        Ok(())
    }

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
    fn encode_len(&self, _value: &bool) -> Result<usize, RocksDbStorageError> {
        Ok(1)
    }

    fn encode_into<W: Write>(&self, value: &bool, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(&[u8::from(*value)])
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow::anyhow!("BoolCodec: Failed to write bool: {}", e),
            })?;
        Ok(())
    }

    fn encode(&self, value: &bool) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::new_from_array([u8::from(*value)]))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<bool, RocksDbStorageError> {
        let mut buf = [0u8; 1];
        reader
            .read_exact(&mut buf)
            .map_err(|e| RocksDbStorageError::MalformedData {
                operation: "decode u8",
                details: format!("Failed to read 1 byte for BoolCodec: {e}"),
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
    fn encode_len(&self, _value: &u8) -> Result<usize, RocksDbStorageError> {
        Ok(1)
    }

    fn encode_into<W: Write>(&self, value: &u8, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(&[*value])
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow::anyhow!("NumberCodec<u8>: Failed to write u8: {}", e),
            })?;
        Ok(())
    }

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
    fn encode_len(&self, _value: &u32) -> Result<usize, RocksDbStorageError> {
        Ok(size_of::<u32>())
    }

    fn encode_into<W: Write>(&self, value: &u32, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(&value.to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow::anyhow!("NumberCodec<u32>: Failed to write u32: {}", e),
            })?;
        Ok(())
    }

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
    fn encode_len(&self, _value: &u64) -> Result<usize, RocksDbStorageError> {
        Ok(size_of::<u64>())
    }

    fn encode_into<W: Write>(&self, value: &u64, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(&value.to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow::anyhow!("NumberCodec<u64>: Failed to write u64: {}", e),
            })?;
        Ok(())
    }

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
    fn encode_len(&self, _value: &Epoch) -> Result<usize, RocksDbStorageError> {
        Ok(size_of::<Epoch>())
    }

    fn encode_into<W: Write>(&self, value: &Epoch, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(&value.to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow::anyhow!("NumberCodec<Epoch>: Failed to write Epoch: {}", e),
            })?;
        Ok(())
    }

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
    fn encode_len(&self, _value: &NodeHeight) -> Result<usize, RocksDbStorageError> {
        Ok(size_of::<NodeHeight>())
    }

    fn encode_into<W: Write>(&self, value: &NodeHeight, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(&value.as_u64().to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow::anyhow!("NumberCodec<NodeHeight>: Failed to write NodeHeight: {}", e),
            })?;
        Ok(())
    }

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
    fn encode_len(&self, _value: &Shard) -> Result<usize, RocksDbStorageError> {
        Ok(size_of::<Shard>())
    }

    fn encode_into<W: Write>(&self, value: &Shard, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(&value.as_u32().to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow::anyhow!("NumberCodec<Shard>: Failed to write Shard: {}", e),
            })?;
        Ok(())
    }

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
