//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Write;

use anyhow::anyhow;
use minicbor::{CborLen, Decode, Decoder, Encode};

use crate::{
    codecs::{DbDecoder, DbEncoder},
    error::RocksDbStorageError,
};

/// Codec that encodes/decodes values using `tari_bor`'s minicbor wire format.
///
/// This is the canonical storage codec for the state store. Types must implement
/// `minicbor::Encode<()>`, `minicbor::Decode<'_, ()>`, and `minicbor::CborLen<()>`
/// (typically via `#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]`
/// with `#[n(N)]` field tags).
#[derive(Debug, Clone, Copy)]
pub struct Minicbor<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> Minicbor<T> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> DbEncoder<T> for Minicbor<T>
where T: Encode<()> + CborLen<()>
{
    fn encode_len(&self, value: &T) -> Result<usize, RocksDbStorageError> {
        Ok(minicbor::len(value))
    }

    fn encode_into<W: Write>(&self, value: &T, writer: &mut W) -> Result<(), RocksDbStorageError> {
        tari_bor::encode_into_writer(value, writer).map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })
    }
}

impl<T> DbDecoder<T> for Minicbor<T>
where T: for<'b> Decode<'b, ()>
{
    fn decode(&self, bytes: &[u8]) -> Result<(T, usize), RocksDbStorageError> {
        let mut decoder = Decoder::new(bytes);
        let value = decoder.decode().map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!(
                "Minicbor decoding failed for type {}: {}",
                std::any::type_name::<T>(),
                e
            ),
        })?;
        Ok((value, decoder.position()))
    }
}

impl<T> Default for Minicbor<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use minicbor::{CborLen, Decode, Encode};

    use super::*;

    #[derive(Debug, PartialEq, Encode, Decode, CborLen)]
    struct TestStruct {
        #[n(0)]
        a: u32,
        #[n(1)]
        b: String,
        #[n(2)]
        c: u64,
    }

    #[test]
    fn it_encodes_and_decodes() {
        let codec = Minicbor::<TestStruct>::new();
        let original = TestStruct {
            a: 42,
            b: "Hello, world!".to_string(),
            c: 9999,
        };

        let mut encoded = Vec::new();
        codec.encode_into(&original, &mut encoded).unwrap();
        let len = codec.encode_len(&original).unwrap();
        assert_eq!(len, encoded.len());
        let decoded = codec.decode_exact(&encoded).unwrap();

        assert_eq!(original, decoded);
    }
}
