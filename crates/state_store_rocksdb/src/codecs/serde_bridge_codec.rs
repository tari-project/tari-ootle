//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::{Read, Write};

use anyhow::anyhow;

use crate::{
    codecs::{DbDecoder, DbEncoder},
    error::RocksDbStorageError,
};

/// Codec for foreign types that only carry `serde` derives (e.g. `tari_jellyfish::Node` and
/// `StaleTreeNode`). Bytes are produced by `minicbor-serde` — RFC-8949-valid CBOR, but with
/// string-keyed maps for struct fields rather than `#[n(N)]` tags, so it lacks the integer-
/// tag size win that hand-derived types get. Acceptable since the only callers are state
/// tree blobs we don't otherwise own.
#[derive(Debug, Clone, Copy)]
pub struct SerdeBridgeCodec<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> SerdeBridgeCodec<T> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> DbEncoder<T> for SerdeBridgeCodec<T>
where T: serde::Serialize
{
    fn encode_len(&self, value: &T) -> Result<usize, RocksDbStorageError> {
        // minicbor-serde doesn't expose a length-only path, so we serialize and discard.
        // Bridged types are state-tree node blobs, encoded sparingly.
        let bytes = minicbor_serde::to_vec(value).map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        Ok(bytes.len())
    }

    fn encode_into<W: Write>(&self, value: &T, writer: &mut W) -> Result<(), RocksDbStorageError> {
        let bytes = minicbor_serde::to_vec(value).map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        writer
            .write_all(&bytes)
            .map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })
    }
}

impl<T> DbDecoder<T> for SerdeBridgeCodec<T>
where T: serde::de::DeserializeOwned
{
    fn decode(&self, bytes: &[u8]) -> Result<T, RocksDbStorageError> {
        minicbor_serde::from_slice(bytes).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!(
                "SerdeBridge decoding failed for type {}: {}",
                std::any::type_name::<T>(),
                e
            ),
        })
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<T, RocksDbStorageError> {
        let mut buf = Vec::new();
        reader
            .read_to_end(&mut buf)
            .map_err(|e| RocksDbStorageError::DecodeError { source: e.into() })?;
        self.decode(&buf)
    }
}

impl<T> Default for SerdeBridgeCodec<T> {
    fn default() -> Self {
        Self::new()
    }
}
