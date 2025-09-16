//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Read;

use anyhow::anyhow;
use serde::Serialize;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
};

const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

fn bincode_encode<T: Serialize>(value: &T) -> Result<Vec<u8>, RocksDbStorageError> {
    let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)
        .map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
    Ok(bytes)
}

#[derive(Debug, Clone, Copy)]
pub struct Bincode<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> Bincode<T> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> DbCodec<T> for Bincode<T>
where T: serde::Serialize + serde::de::DeserializeOwned
{
    fn encode(&self, value: &T) -> Result<EncodeVec, RocksDbStorageError> {
        let bytes = bincode_encode(value)?;
        Ok(bytes.into())
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<T, RocksDbStorageError> {
        bincode::serde::decode_from_std_read(reader, BINCODE_CONFIG).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!(
                "Bincode deserialization failed for type:{}: {}",
                std::any::type_name::<T>(),
                e,
            ),
        })
    }
}

impl<T> Default for Bincode<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BincodeRef<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> BincodeRef<T> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> DbCodec<T> for BincodeRef<T>
where T: serde::Serialize
{
    fn encode(&self, value: &T) -> Result<EncodeVec, RocksDbStorageError> {
        let buf = bincode_encode(value)?;
        Ok(buf.into())
    }

    fn decode_reader<R: Read>(&self, _reader: &mut R) -> Result<T, RocksDbStorageError> {
        panic!("BincodeRef::decode_reader() is not supported. You likely need to use the Bincode codec instead.");
    }
}

impl<T> Default for BincodeRef<T> {
    fn default() -> Self {
        Self::new()
    }
}
