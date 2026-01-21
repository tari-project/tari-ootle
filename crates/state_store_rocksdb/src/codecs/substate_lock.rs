//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::{Read, Write};

use anyhow::anyhow;
use tari_common_types::types::FixedHash;
use tari_consensus_types::BlockId;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::NodeHeight;
use tari_ootle_transaction::TransactionId;

use crate::{
    codecs::{DbCodec, SubstateIdCodec},
    column_families::substate_locks::SubstateLockKey,
    error::RocksDbStorageError,
    utils::read_to_fixed,
};

pub struct SubstateLockKeyCodec<T> {
    substate_id_codec: SubstateIdCodec,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> SubstateLockKeyCodec<T> {
    fn decode_transaction_id<R: Read>(&self, reader: &mut R) -> Result<TransactionId, RocksDbStorageError> {
        let buf = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("SubstateLockKeyCodec: Invalid bytes for FixedHash"),
        })?;
        Ok(TransactionId::new(buf))
    }

    fn decode_block_id<R: Read>(&self, reader: &mut R) -> Result<BlockId, RocksDbStorageError> {
        let buf = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("SubstateLockKeyCodec: Invalid bytes for FixedHash"),
        })?;
        Ok(BlockId::new(FixedHash::new(buf)))
    }

    fn decode_substate_id<R: Read>(&self, reader: &mut R) -> Result<SubstateId, RocksDbStorageError> {
        self.substate_id_codec
            .decode_reader(reader)
            .map_err(|e| RocksDbStorageError::DecodeError {
                source: anyhow!("SubstateLockKeyCodec: Failed to decode SubstateId: {}", e),
            })
    }

    fn decode_block_height<R: Read>(&self, reader: &mut R) -> Result<NodeHeight, RocksDbStorageError> {
        let height = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("SubstateLockKeyCodec: Invalid bytes for NodeHeight"),
        })?;
        Ok(NodeHeight(u64::from_be_bytes(height)))
    }

    fn get_encoded_len(&self, value: &SubstateLockKey) -> Result<usize, RocksDbStorageError> {
        let len = BlockId::byte_size() + // block_id
            self.substate_id_codec.encode_len(&value.substate_id)? + // substate_id
            TransactionId::byte_size() + // transaction_id
            8; // block_height
        Ok(len)
    }
}

impl DbCodec<SubstateLockKey> for SubstateLockKeyCodec<(TransactionId, SubstateId, BlockId, NodeHeight)> {
    fn encode_len(&self, value: &SubstateLockKey) -> Result<usize, RocksDbStorageError> {
        self.get_encoded_len(value)
    }

    fn encode_into<W: Write>(&self, value: &SubstateLockKey, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(value.transaction_id.as_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("SubstateLockKeyCodec: Failed to write transaction_id: {}", e),
            })?;
        self.substate_id_codec.encode_into(&value.substate_id, writer)?;
        writer
            .write_all(value.block_id.as_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("SubstateLockKeyCodec: Failed to write block_id: {}", e),
            })?;
        writer
            .write_all(&value.block_height.as_u64().to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("SubstateLockKeyCodec: Failed to write block_height: {}", e),
            })?;
        Ok(())
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<SubstateLockKey, RocksDbStorageError> {
        let transaction_id = self.decode_transaction_id(reader)?;
        let substate_id = self.decode_substate_id(reader)?;
        let block_id = self.decode_block_id(reader)?;
        let block_height = self.decode_block_height(reader)?;
        Ok(SubstateLockKey {
            block_id,
            substate_id,
            transaction_id,
            block_height,
        })
    }
}

impl DbCodec<SubstateLockKey> for SubstateLockKeyCodec<(BlockId, SubstateId, TransactionId, NodeHeight)> {
    fn encode_len(&self, value: &SubstateLockKey) -> Result<usize, RocksDbStorageError> {
        self.get_encoded_len(value)
    }

    fn encode_into<W: Write>(&self, value: &SubstateLockKey, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(value.block_id.as_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("SubstateLockKeyCodec: Failed to write block_id: {}", e),
            })?;
        self.substate_id_codec.encode_into(&value.substate_id, writer)?;
        writer
            .write_all(value.transaction_id.as_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("SubstateLockKeyCodec: Failed to write transaction_id: {}", e),
            })?;
        writer
            .write_all(&value.block_height.as_u64().to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("SubstateLockKeyCodec: Failed to write block_height: {}", e),
            })?;
        Ok(())
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<SubstateLockKey, RocksDbStorageError> {
        let block_id = self.decode_block_id(reader)?;
        let substate_id = self.decode_substate_id(reader)?;
        let transaction_id = self.decode_transaction_id(reader)?;
        let block_height = self.decode_block_height(reader)?;

        Ok(SubstateLockKey {
            block_id,
            substate_id,
            transaction_id,
            block_height,
        })
    }
}

impl DbCodec<SubstateLockKey> for SubstateLockKeyCodec<(SubstateId, TransactionId, BlockId, NodeHeight)> {
    fn encode_len(&self, value: &SubstateLockKey) -> Result<usize, RocksDbStorageError> {
        self.get_encoded_len(value)
    }

    fn encode_into<W: Write>(&self, value: &SubstateLockKey, writer: &mut W) -> Result<(), RocksDbStorageError> {
        writer
            .write_all(&self.substate_id_codec.encode(&value.substate_id)?)
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("SubstateLockKeyCodec: Failed to write substate_id: {}", e),
            })?;
        writer
            .write_all(value.transaction_id.as_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("SubstateLockKeyCodec: Failed to write transaction_id: {}", e),
            })?;
        writer
            .write_all(value.block_id.as_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("SubstateLockKeyCodec: Failed to write block_id: {}", e),
            })?;
        writer
            .write_all(&value.block_height.as_u64().to_be_bytes())
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("SubstateLockKeyCodec: Failed to write block_height: {}", e),
            })?;
        Ok(())
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<SubstateLockKey, RocksDbStorageError> {
        let substate_id = self.decode_substate_id(reader)?;
        let transaction_id = self.decode_transaction_id(reader)?;
        let block_id = self.decode_block_id(reader)?;
        let block_height = self.decode_block_height(reader)?;

        Ok(SubstateLockKey {
            block_id,
            substate_id,
            transaction_id,
            block_height,
        })
    }
}

impl<T> Default for SubstateLockKeyCodec<T> {
    fn default() -> Self {
        Self {
            substate_id_codec: SubstateIdCodec,
            _phantom: Default::default(),
        }
    }
}
