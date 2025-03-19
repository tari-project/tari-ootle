//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use tari_common_types::types::FixedHash;
use tari_dan_storage::consensus_models::BlockId;

use crate::{
    codecs::{DbCodec, EncodeVec, SubstateIdCodec},
    error::RocksDbStorageError,
    model::block_diff::BlockDiffKey,
    utils::read_to_fixed,
};

#[derive(Default)]
pub struct BlockIdSubstateIdVersionCodec {
    substate_id_codec: SubstateIdCodec,
}

impl DbCodec<BlockDiffKey> for BlockIdSubstateIdVersionCodec {
    fn encode(&self, value: &BlockDiffKey) -> Result<EncodeVec, RocksDbStorageError> {
        let block_id_bytes = value.block_id.as_bytes();
        let substate_id_bytes = self.substate_id_codec.encode(&value.substate_id)?;
        let version_bytes = value.version.to_be_bytes();
        Ok(EncodeVec::from_slices(&[
            block_id_bytes,
            &substate_id_bytes,
            &version_bytes,
        ]))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<BlockDiffKey, RocksDbStorageError> {
        let reader = &mut bytes;
        let hash = FixedHash::new(read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!(
                "BlockIdSubstateIdVersionCodec: Invalid bytes len={} for FixedHash",
                reader.len()
            ),
        })?);
        let substate_id = self.substate_id_codec.decode_reader(reader)?;
        let version = u32::from_be_bytes(read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!(
                "BlockIdSubstateIdVersionCodec: Invalid bytes len={} for u32",
                reader.len()
            ),
        })?);
        Ok(BlockDiffKey {
            block_id: BlockId::from(hash),
            substate_id,
            version,
        })
    }
}

#[derive(Default)]
pub struct BlockDiffKeyCodec {
    substate_id_codec: SubstateIdCodec,
}

impl DbCodec<BlockDiffKey> for BlockDiffKeyCodec {
    fn encode(&self, value: &BlockDiffKey) -> Result<EncodeVec, RocksDbStorageError> {
        let substate_id_bytes = self.substate_id_codec.encode(&value.substate_id)?;
        let block_id_bytes = value.block_id.as_bytes();
        let version_bytes = value.version.to_be_bytes();
        Ok(EncodeVec::from_slices(&[
            &substate_id_bytes,
            block_id_bytes,
            &version_bytes,
        ]))
    }

    fn decode(&self, mut bytes: &[u8]) -> Result<BlockDiffKey, RocksDbStorageError> {
        let reader = &mut bytes;
        let substate_id = self.substate_id_codec.decode_reader(reader)?;
        let hash = read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!("BlockIdSubstateIdVersionCodec: Invalid bytes for FixedHash",),
        })?;
        let hash = FixedHash::new(hash);
        let version = u32::from_be_bytes(read_to_fixed(reader).ok_or_else(|| RocksDbStorageError::DecodeError {
            source: anyhow!(
                "BlockIdSubstateIdVersionCodec: Invalid bytes len={} for u32",
                reader.len()
            ),
        })?);
        Ok(BlockDiffKey {
            block_id: BlockId::from(hash),
            substate_id,
            version,
        })
    }
}
