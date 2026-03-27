//  Copyright 2022. The Tari Project
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

use serde::{Deserialize, Serialize};
use tari_template_abi::rust::{
    fmt,
    fmt::{Display, Formatter},
    ops::{Deref, DerefMut},
    prelude::*,
    str::FromStr,
};

use crate::{
    hex::{fixed_bytes_from_hex, write_hex_fmt},
    serde_helpers,
};

/// Representation of a 32-byte hash value
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Hash32(
    #[serde(with = "serde_helpers::fixed_hex")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    [u8; Self::LENGTH],
);

impl Hash32 {
    pub const LENGTH: usize = 32;

    pub const fn zero() -> Self {
        Self([0u8; Self::LENGTH])
    }

    pub const fn from_array(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    pub const fn into_array(self) -> [u8; Self::LENGTH] {
        self.0
    }

    pub const fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn from_hex(s: &str) -> Result<Self, HashParseError> {
        let hash = fixed_bytes_from_hex(s).map_err(|_| HashParseError)?;
        Ok(Hash32(hash))
    }

    pub fn write_hex_fmt<W: fmt::Write>(&self, writer: &mut W) -> fmt::Result {
        write_hex_fmt(writer, &self.0)
    }

    pub fn try_from_slice(data: &[u8]) -> Result<Self, HashParseError> {
        Self::try_from(data)
    }

    /// Returns the leading `N` bytes of the hash
    ///
    /// # Panics
    ///
    /// Panics if `N` is greater than Self::LENGTH (32)
    pub fn leading_bytes<const N: usize>(&self) -> [u8; N] {
        self.0
            .get(..N)
            .expect("invariant violation: N > Self::LENGTH")
            .try_into()
            .unwrap()
    }

    /// Returns the trailing `N` bytes of the hash
    ///
    /// # Panics
    ///
    /// Panics if `N` is greater than Self::LENGTH (32)
    pub fn trailing_bytes<const N: usize>(&self) -> [u8; N] {
        self.0
            .get((Self::LENGTH - N)..Self::LENGTH)
            .expect("invariant violation: N > Self::LENGTH")
            .try_into()
            .unwrap()
    }
}

impl AsRef<[u8]> for Hash32 {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<[u8; Self::LENGTH]> for Hash32 {
    fn from(hash: [u8; Self::LENGTH]) -> Self {
        Self::from_array(hash)
    }
}

impl FromStr for Hash32 {
    type Err = HashParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Hash32::from_hex(s)
    }
}

impl TryFrom<&[u8]> for Hash32 {
    type Error = HashParseError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != Self::LENGTH {
            return Err(HashParseError);
        }
        let mut hash = [0u8; Self::LENGTH];
        hash.copy_from_slice(value);
        Ok(Hash32::from_array(hash))
    }
}

impl TryFrom<Vec<u8>> for Hash32 {
    type Error = HashParseError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Hash32::try_from(value.as_slice())
    }
}

impl Deref for Hash32 {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Hash32 {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Display for Hash32 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write_hex_fmt(f, &self.0)
    }
}

/// Representation of a hash parsing error
#[derive(Debug)]
pub struct HashParseError;

#[cfg(feature = "std")]
impl std::error::Error for HashParseError {}

impl Display for HashParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to parse hash")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_deserialize() {
        let hash = Hash32::default();
        let mut buf = Vec::new();
        tari_bor::encode_into_writer(&hash, &mut buf).unwrap();
        let hash2 = tari_bor::decode(&buf).unwrap();
        assert_eq!(hash, hash2);
    }
}
