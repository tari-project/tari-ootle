//  Copyright 2025. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::io;

use anyhow::anyhow;
use serde::{de::DeserializeOwned, Serialize};

use crate::error::RocksDbStorageError;

const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

pub fn bincode_encode<T: Serialize>(value: &T) -> Result<Vec<u8>, RocksDbStorageError> {
    let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)
        .map_err(|e| RocksDbStorageError::EncodeError { source: e.into() })?;
    Ok(bytes)
}

pub fn bincode_decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, RocksDbStorageError> {
    let (value, bytes_read) =
        bincode::serde::decode_from_slice(bytes, BINCODE_CONFIG).map_err(|e| RocksDbStorageError::DecodeError {
            source: anyhow!(
                "Bincode deserialization failed for type:{}: {}",
                std::any::type_name::<T>(),
                e,
            ),
        })?;
    if bytes_read != bytes.len() {
        return Err(RocksDbStorageError::DecodeError {
            source: anyhow!(
                "Bincode deserialization failed for type {}. Bytes read: {}. Bytes length: {}",
                std::any::type_name::<T>(),
                bytes_read,
                bytes.len()
            ),
        });
    }
    Ok(value)
}

pub(crate) fn read_to_fixed<const SZ: usize, T, R>(reader: &mut R) -> Option<T>
where
    [u8; SZ]: Into<T>,
    R: io::Read,
{
    let mut array = [0u8; SZ];
    reader.read_exact(&mut array).ok()?;
    Some(array.into())
}

pub(crate) fn read_n_bytes<R: io::Read>(reader: &mut R, n: usize) -> Option<Vec<u8>> {
    let mut vec = vec![0u8; n];
    reader.read_exact(&mut vec[..]).ok()?;
    Some(vec)
}
