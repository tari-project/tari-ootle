//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::{Read, Write};

use crate::{codecs::DbCodec, error::RocksDbStorageError};

#[derive(Debug, Clone, Copy)]
pub struct BorshCodec<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> BorshCodec<T> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> DbCodec<T> for BorshCodec<T>
where T: borsh::BorshSerialize + borsh::BorshDeserialize
{
    fn encode_len(&self, value: &T) -> Result<usize, RocksDbStorageError> {
        let len = borsh::object_length(value).map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        Ok(len)
    }

    fn encode_into<W: Write>(&self, value: &T, writer: &mut W) -> Result<(), RocksDbStorageError> {
        borsh::to_writer(writer, value).map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
        Ok(())
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<T, RocksDbStorageError> {
        borsh::from_reader(reader).map_err(|e| RocksDbStorageError::DecodeError { source: e.into() })
    }
}

impl<T> Default for BorshCodec<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_encodes_and_decodes() {
        #[derive(borsh::BorshSerialize, borsh::BorshDeserialize, PartialEq, Debug)]
        struct TestStruct {
            a: u32,
            b: String,
            c: u64,
        }

        let codec = BorshCodec::<TestStruct>::new();
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
