//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
    utils::{bincode_decode, bincode_encode},
};

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
        let buf = bincode_encode(value)?;
        Ok(buf.into())
    }

    fn decode(&self, bytes: &[u8]) -> Result<T, RocksDbStorageError> {
        let ret = bincode_decode(bytes)?;
        Ok(ret)
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

    fn decode(&self, _bytes: &[u8]) -> Result<T, RocksDbStorageError> {
        panic!("BincodeRef::decode() is not supported. You likely need to use the Bincode codec instead.");
    }
}

impl<T> Default for BincodeRef<T> {
    fn default() -> Self {
        Self::new()
    }
}
