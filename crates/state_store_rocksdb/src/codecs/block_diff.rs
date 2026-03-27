//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{io, io::Read};

use anyhow::anyhow;
use tari_common_types::types::FixedHash;
use tari_consensus_types::BlockId;

use crate::{
    codecs::{DbDecoder, DbEncoder, SubstateIdCodec},
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

impl DbEncoder<BlockDiffKey> for BlockDiffKeyCodec<BlockIdSeqSubstateIdVersion> {
    fn encode_len(&self, value: &BlockDiffKey) -> Result<usize, RocksDbStorageError> {
        let len = BlockId::byte_size() + // block_id
            4 + // sequence
            self.substate_id_codec.encode_len(&value.substate_id)? + // substate_id
            4 + // version
            1; // is_up
        Ok(len)
    }

    fn encode_into<W: io::Write>(&self, value: &BlockDiffKey, writer: &mut W) -> Result<(), RocksDbStorageError> {
        let block_id_bytes = value.block_id.as_bytes();
        writer
            .write_all(block_id_bytes)
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("BlockIdSubstateIdVersionCodec: Failed to write block_id: {}", e),
            })?;
        let sequence_bytes = value.sequence.to_be_bytes();
        writer
            .write_all(&sequence_bytes)
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("BlockIdSubstateIdVersionCodec: Failed to write sequence: {}", e),
            })?;
        self.substate_id_codec.encode_into(&value.substate_id, writer)?;
        let version_bytes = value.version.to_be_bytes();
        writer
            .write_all(&version_bytes)
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("BlockIdSubstateIdVersionCodec: Failed to write version: {}", e),
            })?;
        let is_up_byte = u8::from(value.is_up);
        writer
            .write_all(&[is_up_byte])
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("BlockIdSubstateIdVersionCodec: Failed to write is_up: {}", e),
            })?;
        Ok(())
    }
}

impl DbDecoder<BlockDiffKey> for BlockDiffKeyCodec<BlockIdSeqSubstateIdVersion> {
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

impl DbEncoder<BlockDiffKey> for BlockDiffKeyCodec<SubstateIdBlockIdVersionSeq> {
    fn encode_len(&self, value: &BlockDiffKey) -> Result<usize, RocksDbStorageError> {
        let len = self.substate_id_codec.encode_len(&value.substate_id)? + // substate_id
            BlockId::byte_size() + // block_id
            4 + // version
            1 + // is_up
            4; // sequence
        Ok(len)
    }

    fn encode_into<W: io::Write>(&self, value: &BlockDiffKey, writer: &mut W) -> Result<(), RocksDbStorageError> {
        let substate_id_bytes = self.substate_id_codec.encode(&value.substate_id)?;
        writer
            .write_all(&substate_id_bytes)
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("BlockIdSubstateIdVersionCodec: Failed to write substate_id: {}", e),
            })?;
        let block_id_bytes = value.block_id.as_bytes();
        writer
            .write_all(block_id_bytes)
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("BlockIdSubstateIdVersionCodec: Failed to write block_id: {}", e),
            })?;
        let version_bytes = value.version.to_be_bytes();
        writer
            .write_all(&version_bytes)
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("BlockIdSubstateIdVersionCodec: Failed to write version: {}", e),
            })?;
        let is_up_byte = u8::from(value.is_up);
        writer
            .write_all(&[is_up_byte])
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("BlockIdSubstateIdVersionCodec: Failed to write is_up: {}", e),
            })?;
        let sequence_bytes = value.sequence.to_be_bytes();
        writer
            .write_all(&sequence_bytes)
            .map_err(|e| RocksDbStorageError::EncodeError {
                source: anyhow!("BlockIdSubstateIdVersionCodec: Failed to write sequence: {}", e),
            })?;
        Ok(())
    }
}

impl DbDecoder<BlockDiffKey> for BlockDiffKeyCodec<SubstateIdBlockIdVersionSeq> {
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
