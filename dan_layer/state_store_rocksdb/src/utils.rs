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

use std::{
    fmt::{self, Display},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::error::RocksDbStorageError;

const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

pub fn bincode_encode<T: Serialize>(value: &T) -> Result<Vec<u8>, RocksDbStorageError> {
    let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
    Ok(bytes)
}

pub fn bincode_decode<T: DeserializeOwned>(bytes: Vec<u8>) -> Result<T, RocksDbStorageError> {
    let (value, _): (T, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
    Ok(value)
}

pub fn bor_encode<T: Serialize>(value: &T) -> Result<Vec<u8>, RocksDbStorageError> {
    tari_bor::encode(value).map_err(|e| RocksDbStorageError::GeneralError {
        message: e.into_string(),
    })
}

pub fn bor_decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, RocksDbStorageError> {
    tari_bor::decode_exact(bytes).map_err(|e| RocksDbStorageError::GeneralError {
        message: e.into_string(),
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, PartialOrd, Ord)]
pub struct RocksdbTimestamp(u128);

impl RocksdbTimestamp {
    pub fn now() -> Self {
        Self(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis())
    }
}

impl Display for RocksdbTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // displayed as binary to order if the value is used in a RocksDB key
        write!(f, "{:b}", self.0)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Eq, PartialEq, PartialOrd, Ord)]
pub struct RocksdbSeq(pub u64);

impl Display for RocksdbSeq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // hexadecimal endcoding with full 0 padding, so the key preserves ordering
        write!(f, "{:#018x}", self.0)
    }
}
