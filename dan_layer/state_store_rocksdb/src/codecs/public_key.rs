//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct PublicKeyCodec;

impl DbCodec<RistrettoPublicKeyBytes> for PublicKeyCodec {
    fn encode(&self, value: &RistrettoPublicKeyBytes) -> Result<EncodeVec, RocksDbStorageError> {
        Ok(EncodeVec::from_slice(value.as_bytes()))
    }

    fn decode(&self, bytes: &[u8]) -> Result<RistrettoPublicKeyBytes, RocksDbStorageError> {
        RistrettoPublicKeyBytes::from_bytes(bytes).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!("RistrettoPublicKeyBytes: {}", e),
        })
    }
}
