//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::{Read, Write};

use anyhow::anyhow;

use crate::{
    codecs::{byte_counter::ByteCounter, DbCodec},
    error::RocksDbStorageError,
};

const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

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
    fn encode_len(&self, value: &T) -> Result<usize, RocksDbStorageError> {
        let mut counter = ByteCounter::new();
        bincode::serde::encode_into_std_write(value, &mut counter, BINCODE_CONFIG)
            .map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        Ok(counter.get())
    }

    fn encode_into<W: Write>(&self, value: &T, writer: &mut W) -> Result<(), RocksDbStorageError> {
        bincode::serde::encode_into_std_write(value, writer, BINCODE_CONFIG)
            .map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        Ok(())
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<T, RocksDbStorageError> {
        bincode::serde::decode_from_std_read(reader, BINCODE_CONFIG).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!(
                "Bincode deserialization failed for type: {}: {}",
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
    fn encode_len(&self, value: &T) -> Result<usize, RocksDbStorageError> {
        let mut counter = ByteCounter::new();
        bincode::serde::encode_into_std_write(value, &mut counter, BINCODE_CONFIG)
            .map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        Ok(counter.get())
    }

    fn encode_into<W: Write>(&self, value: &T, writer: &mut W) -> Result<(), RocksDbStorageError> {
        bincode::serde::encode_into_std_write(value, writer, BINCODE_CONFIG)
            .map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        Ok(())
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

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn it_encodes_and_decodes() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct TestStruct {
            a: u32,
            b: String,
            c: u64,
        }

        let codec = Bincode::<TestStruct>::new();
        let original = TestStruct {
            a: 42,
            b: "Hello, world!".to_string(),
            c: 9999,
        };

        let mut encoded = Vec::new();
        codec.encode_into(&original, &mut encoded).unwrap();
        let len = codec.encode_len(&original).unwrap();
        assert_eq!(len, encoded.len());
        let decoded = codec.decode_reader(&mut &encoded[..]).unwrap();

        assert_eq!(original, decoded);
    }
}
