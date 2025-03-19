//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use tari_dan_common_types::Epoch;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
    model::bookkeeping::BookkeepingKey,
    utils::checked_copy_fixed,
};

const BYTE_SIZE: usize = 1 + size_of::<u64>();

#[derive(Debug, Clone, Copy, Default)]
pub struct BookkeepingKeyCodec;

impl DbCodec<BookkeepingKey> for BookkeepingKeyCodec {
    fn encode(&self, value: &BookkeepingKey) -> Result<EncodeVec, RocksDbStorageError> {
        let mut buf = EncodeVec::make_stack_buf();
        buf[0] = value.as_byte();
        if let Some(epoch) = value.epoch() {
            buf[1..BYTE_SIZE].copy_from_slice(&epoch.to_be_bytes());
        }
        Ok(EncodeVec::new_stack(buf, BYTE_SIZE))
    }

    fn decode(&self, bytes: &[u8]) -> Result<BookkeepingKey, RocksDbStorageError> {
        if bytes.len() < BYTE_SIZE {
            return Err(RocksDbStorageError::DecodeError {
                source: anyhow!("Invalid bytes size {} for BookkeepingCodec", bytes.len()),
            });
        }

        // These must match BookkeepingDiscriminator::as_byte!
        match bytes[0] {
            0 => Ok(BookkeepingKey::LastVoted),
            1 => Ok(BookkeepingKey::LastExecuted),
            2 => Ok(BookkeepingKey::LastProposed),
            3 => Ok(BookkeepingKey::LastSentVote),
            4 => Ok(BookkeepingKey::CommitBlock),
            5 => Ok(BookkeepingKey::LockedBlock(decode_epoch(&bytes[1..])?)),
            6 => Ok(BookkeepingKey::LeafBlock(decode_epoch(&bytes[1..])?)),
            7 => Ok(BookkeepingKey::HighQc(decode_epoch(&bytes[1..])?)),
            _ => Err(RocksDbStorageError::DecodeError {
                source: anyhow!("Invalid BookkeepingDiscriminator value {}", bytes[0]),
            }),
        }
    }
}

fn decode_epoch(bytes: &[u8]) -> Result<Epoch, RocksDbStorageError> {
    let bytes = checked_copy_fixed(bytes).ok_or_else(|| RocksDbStorageError::DecodeError {
        source: anyhow!("Invalid bytes size {} for Epoch", bytes.len()),
    })?;
    Ok(Epoch(u64::from_be_bytes(bytes)))
}
