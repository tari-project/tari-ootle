//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    fmt::{Display, Formatter},
    ops::{Deref, DerefMut},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use tari_template_lib::types::hex::write_hex_fmt;

/// Representation of a 64-byte hash value
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Ord,
    PartialOrd,
    Hash,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
)]
#[cbor(transparent)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Hash64(
    #[n(0)]
    #[cbor(with = "minicbor::bytes")]
    #[serde(with = "ootle_serde::hex")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    [u8; Self::LENGTH],
);

impl Hash64 {
    pub const LENGTH: usize = 64;

    pub const fn zero() -> Self {
        Self::from_array([0u8; Self::LENGTH])
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
        if s.len() != Self::LENGTH * 2 {
            return Err(HashParseError::InvalidLength);
        }
        let mut buf = [0u8; Self::LENGTH];
        hex::decode_to_slice(s, &mut buf)?;
        Ok(Hash64(buf))
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
    /// Panics if `N` is greater than Self::LENGTH (64)
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
    /// Panics if `N` is greater than Self::LENGTH (64)
    pub fn trailing_bytes<const N: usize>(&self) -> [u8; N] {
        self.0
            .get((Self::LENGTH - N)..Self::LENGTH)
            .expect("invariant violation: N > Self::LENGTH")
            .try_into()
            .unwrap()
    }
}

impl AsRef<[u8]> for Hash64 {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<[u8; Self::LENGTH]> for Hash64 {
    fn from(hash: [u8; Self::LENGTH]) -> Self {
        Self::from_array(hash)
    }
}

impl FromStr for Hash64 {
    type Err = HashParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Hash64::from_hex(s)
    }
}

impl TryFrom<&[u8]> for Hash64 {
    type Error = HashParseError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != Self::LENGTH {
            return Err(HashParseError::InvalidLength);
        }
        let mut hash = [0u8; Self::LENGTH];
        hash.copy_from_slice(value);
        Ok(Hash64::from_array(hash))
    }
}

impl TryFrom<Vec<u8>> for Hash64 {
    type Error = HashParseError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Hash64::try_from(value.as_slice())
    }
}

impl Deref for Hash64 {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Hash64 {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Default for Hash64 {
    fn default() -> Self {
        Self::zero()
    }
}

impl Display for Hash64 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write_hex_fmt(f, &self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HashParseError {
    #[error("Invalid hex string: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("Invalid length")]
    InvalidLength,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_deserialize() {
        let hash = Hash64::default();
        let mut buf = Vec::new();
        tari_bor::encode_into_writer(&hash, &mut buf).unwrap();
        let val = tari_bor::to_value(&hash).unwrap();
        assert_eq!(val, tari_bor::Value::Bytes(vec![0u8; Hash64::LENGTH]));
        let hash2 = tari_bor::decode(&buf).unwrap();
        assert_eq!(hash, hash2);
    }
}
