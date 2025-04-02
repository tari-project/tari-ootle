//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use tari_common_types::types::FixedHash;
use tari_dan_storage::consensus_models::BlockId;
use tari_engine_types::substate::SubstateId;
use tari_transaction::TransactionId;

use crate::{
    codecs::{DbCodec, EncodeVec, SubstateIdCodec},
    error::RocksDbStorageError,
    model::substate_locks::SubstateLockKey,
    utils::read_to_fixed,
};

pub struct SubstateLockKeyCodec<T> {
    substate_id_codec: SubstateIdCodec,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> SubstateLockKeyCodec<T> {
    fn decode_transaction_id(&self, reader: &mut &[u8]) -> Result<TransactionId, RocksDbStorageError> {
        let buf = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("SubstateLockKeyCodec: Invalid bytes len={} for FixedHash", reader.len()),
        })?;
        Ok(TransactionId::new(buf))
    }

    fn decode_block_id(&self, reader: &mut &[u8]) -> Result<BlockId, RocksDbStorageError> {
        let buf = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("SubstateLockKeyCodec: Invalid bytes len={} for FixedHash", reader.len()),
        })?;
        Ok(BlockId::new(FixedHash::new(buf)))
    }

    fn decode_substate_id(&self, reader: &mut &[u8]) -> Result<SubstateId, RocksDbStorageError> {
        let substate_id = self.substate_id_codec.decode_reader(reader)?;
        Ok(substate_id)
    }
}

impl DbCodec<SubstateLockKey> for SubstateLockKeyCodec<(TransactionId, SubstateId, BlockId)> {
    fn encode(&self, value: &SubstateLockKey) -> Result<EncodeVec, RocksDbStorageError> {
        let tx_id_bytes = value.transaction_id.as_bytes();
        let block_id_bytes = value.block_id.as_bytes();
        let substate_id_bytes = self.substate_id_codec.encode(&value.substate_id)?;
        Ok(EncodeVec::from_slices(&[
            tx_id_bytes,
            &substate_id_bytes,
            block_id_bytes,
        ]))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<SubstateLockKey, RocksDbStorageError> {
        let reader = &mut bytes;
        let transaction_id = self.decode_transaction_id(reader)?;
        let substate_id = self.decode_substate_id(reader)?;
        let block_id = self.decode_block_id(reader)?;
        Ok(SubstateLockKey {
            block_id,
            substate_id,
            transaction_id,
        })
    }
}

impl DbCodec<SubstateLockKey> for SubstateLockKeyCodec<(BlockId, SubstateId, TransactionId)> {
    fn encode(&self, value: &SubstateLockKey) -> Result<EncodeVec, RocksDbStorageError> {
        let tx_id_bytes = value.transaction_id.as_bytes();
        let block_id_bytes = value.block_id.as_bytes();
        let substate_id_bytes = self.substate_id_codec.encode(&value.substate_id)?;
        Ok(EncodeVec::from_slices(&[
            block_id_bytes,
            &substate_id_bytes,
            tx_id_bytes,
        ]))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<SubstateLockKey, RocksDbStorageError> {
        let reader = &mut bytes;
        let block_id = self.decode_block_id(reader)?;
        let substate_id = self.decode_substate_id(reader)?;
        let transaction_id = self.decode_transaction_id(reader)?;

        Ok(SubstateLockKey {
            block_id,
            substate_id,
            transaction_id,
        })
    }
}

impl DbCodec<SubstateLockKey> for SubstateLockKeyCodec<(SubstateId, TransactionId, BlockId)> {
    fn encode(&self, value: &SubstateLockKey) -> Result<EncodeVec, RocksDbStorageError> {
        let tx_id_bytes = value.transaction_id.as_bytes();
        let block_id_bytes = value.block_id.as_bytes();
        let substate_id_bytes = self.substate_id_codec.encode(&value.substate_id)?;
        Ok(EncodeVec::from_slices(&[
            &substate_id_bytes,
            tx_id_bytes,
            block_id_bytes,
        ]))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<SubstateLockKey, RocksDbStorageError> {
        let reader = &mut bytes;
        let substate_id = self.decode_substate_id(reader)?;
        let transaction_id = self.decode_transaction_id(reader)?;
        let block_id = self.decode_block_id(reader)?;

        Ok(SubstateLockKey {
            block_id,
            substate_id,
            transaction_id,
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
