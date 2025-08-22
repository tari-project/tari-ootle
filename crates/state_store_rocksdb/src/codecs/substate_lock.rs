//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Read;

use anyhow::anyhow;
use tari_common_types::types::FixedHash;
use tari_consensus_types::BlockId;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::NodeHeight;
use tari_transaction::TransactionId;

use crate::{
    codecs::{DbCodec, EncodeVec, SubstateIdCodec},
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
        self.substate_id_codec.decode_reader(reader)
    }

    fn decode_block_height<R: Read>(&self, reader: &mut R) -> Result<NodeHeight, RocksDbStorageError> {
        let height = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("SubstateLockKeyCodec: Invalid bytes for NodeHeight"),
        })?;
        Ok(NodeHeight(u64::from_be_bytes(height)))
    }
}

impl DbCodec<SubstateLockKey> for SubstateLockKeyCodec<(TransactionId, SubstateId, BlockId, NodeHeight)> {
    fn encode(&self, value: &SubstateLockKey) -> Result<EncodeVec, RocksDbStorageError> {
        let tx_id_bytes = value.transaction_id.as_bytes();
        let block_id_bytes = value.block_id.as_bytes();
        let substate_id_bytes = self.substate_id_codec.encode(&value.substate_id)?;
        let block_height_bytes = value.block_height.as_u64().to_be_bytes();
        Ok(EncodeVec::from_slices(&[
            tx_id_bytes,
            &substate_id_bytes,
            block_id_bytes,
            &block_height_bytes,
        ]))
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
    fn encode(&self, value: &SubstateLockKey) -> Result<EncodeVec, RocksDbStorageError> {
        let tx_id_bytes = value.transaction_id.as_bytes();
        let block_id_bytes = value.block_id.as_bytes();
        let substate_id_bytes = self.substate_id_codec.encode(&value.substate_id)?;
        let block_height_bytes = value.block_height.as_u64().to_be_bytes();
        Ok(EncodeVec::from_slices(&[
            block_id_bytes,
            &substate_id_bytes,
            tx_id_bytes,
            &block_height_bytes,
        ]))
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
    fn encode(&self, value: &SubstateLockKey) -> Result<EncodeVec, RocksDbStorageError> {
        let tx_id_bytes = value.transaction_id.as_bytes();
        let block_id_bytes = value.block_id.as_bytes();
        let substate_id_bytes = self.substate_id_codec.encode(&value.substate_id)?;
        let block_height_bytes = value.block_height.as_u64().to_be_bytes();
        Ok(EncodeVec::from_slices(&[
            &substate_id_bytes,
            tx_id_bytes,
            block_id_bytes,
            &block_height_bytes,
        ]))
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
