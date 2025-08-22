//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Read;

use anyhow::anyhow;
use tari_common_types::types::FixedHash;
use tari_consensus_types::BlockId;

use crate::{
    codecs::{DbCodec, EncodeVec, SubstateIdCodec},
    column_families::block_diff::BlockDiffKey,
    error::RocksDbStorageError,
    utils::read_to_fixed,
};

/// Variant for BlockDiffKeyCodec that encodes in block_id, seq, substate_id, and version order.
pub struct BlockIdSeqSubstateIdVersion;
/// Variant for BlockDiffKeyCodec that encodes in substate_id, block_id, version, and seq order.
pub struct SubstateIdBlockIdVersionSeq;

pub struct BlockDiffKeyCodec<T> {
    substate_id_codec: SubstateIdCodec,
    _phantom: std::marker::PhantomData<T>,
}

impl DbCodec<BlockDiffKey> for BlockDiffKeyCodec<BlockIdSeqSubstateIdVersion> {
    fn encode(&self, value: &BlockDiffKey) -> Result<EncodeVec, RocksDbStorageError> {
        let block_id_bytes = value.block_id.as_bytes();
        let sequence_bytes = value.sequence.to_be_bytes();
        let substate_id_bytes = self.substate_id_codec.encode(&value.substate_id)?;
        let version_bytes = value.version.to_be_bytes();
        let bool_byte = [u8::from(value.is_up)];
        Ok(EncodeVec::from_slices(&[
            block_id_bytes,
            &sequence_bytes,
            &substate_id_bytes,
            &version_bytes,
            bool_byte.as_slice(),
        ]))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<BlockDiffKey, RocksDbStorageError> {
        let block_id = FixedHash::new(read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("BlockIdSubstateIdVersionCodec: Invalid bytes for FixedHash",),
        })?);
        let sequence = u32::from_be_bytes(read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("BlockIdSubstateIdVersionCodec: not enough bytes for sequence"),
        })?);
        let substate_id = self.substate_id_codec.decode_reader(reader)?;
        let version = u32::from_be_bytes(read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("BlockIdSubstateIdVersionCodec: Invalid bytes for u32",),
        })?);
        let is_up: [u8; 1] = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("BlockIdSubstateIdVersionCodec: not enough bytes for bool",),
        })?;

        Ok(BlockDiffKey {
            block_id: BlockId::from(block_id),
            substate_id,
            version,
            is_up: is_up[0] != 0,
            sequence,
        })
    }
}

impl DbCodec<BlockDiffKey> for BlockDiffKeyCodec<SubstateIdBlockIdVersionSeq> {
    fn encode(&self, value: &BlockDiffKey) -> Result<EncodeVec, RocksDbStorageError> {
        let substate_id_bytes = self.substate_id_codec.encode(&value.substate_id)?;
        let block_id_bytes = value.block_id.as_bytes();
        let version_bytes = value.version.to_be_bytes();
        let is_up_bytes = [u8::from(value.is_up)];
        let sequence_bytes = value.sequence.to_be_bytes();
        Ok(EncodeVec::from_slices(&[
            &substate_id_bytes,
            block_id_bytes,
            &version_bytes,
            is_up_bytes.as_slice(),
            &sequence_bytes,
        ]))
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<BlockDiffKey, RocksDbStorageError> {
        let substate_id = self.substate_id_codec.decode_reader(reader)?;
        let block_id = FixedHash::new(read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("BlockIdSubstateIdVersionCodec: Invalid bytes for FixedHash",),
        })?);
        let version = u32::from_be_bytes(read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("BlockIdSubstateIdVersionCodec: Invalid bytes for u32",),
        })?);
        let is_up: [u8; 1] = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("BlockIdSubstateIdVersionCodec: not enough bytes for bool"),
        })?;
        let sequence = u32::from_be_bytes(read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("BlockIdSubstateIdVersionCodec: not enough bytes for sequence",),
        })?);
        Ok(BlockDiffKey {
            block_id: BlockId::from(block_id),
            substate_id,
            version,
            is_up: is_up[0] != 0,
            sequence,
        })
    }
}

impl<T> Default for BlockDiffKeyCodec<T> {
    fn default() -> Self {
        Self {
            substate_id_codec: SubstateIdCodec,
            _phantom: std::marker::PhantomData,
        }
    }
}
