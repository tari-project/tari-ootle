//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use tari_common_types::types::PublicKey;
use tari_utilities::ByteArray;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct PublicKeyCodec;

impl DbCodec<PublicKey> for PublicKeyCodec {
    fn encode(&self, value: &PublicKey) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::from_slice(value.as_bytes()))
    }

    fn decode(&self, bytes: &[u8]) -> Result<PublicKey, RocksDbStorageError> {
        PublicKey::from_canonical_bytes(bytes).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!("PublicKey: {}", e),
        })
    }
}
