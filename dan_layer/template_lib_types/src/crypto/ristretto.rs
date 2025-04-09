//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_abi::rust::{
    fmt::{Display, Formatter},
    ops::Deref,
    str::FromStr,
};

use crate::{crypto::InvalidByteLengthError, serde_helpers, serde_helpers::fixed_bytes_from_hex, Hash, HashParseError};

/// A Ristretto public key byte contents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash, Default, Serialize, Deserialize)]
pub struct RistrettoPublicKeyBytes(#[serde(with = "serde_helpers::fixed_hex")] [u8; RistrettoPublicKeyBytes::length()]);

impl RistrettoPublicKeyBytes {
    pub const fn length() -> usize {
        32
    }

    pub fn zero() -> Self {
        Self([0u8; Self::length()])
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, InvalidByteLengthError> {
        if bytes.len() != Self::length() {
            return Err(InvalidByteLengthError {
                size: bytes.len(),
                expected: Self::length(),
            });
        }

        let mut key = [0u8; Self::length()];
        key.copy_from_slice(bytes);
        Ok(RistrettoPublicKeyBytes(key))
    }

    pub fn from_hex(hex: &str) -> Result<Self, HashParseError> {
        let bytes = fixed_bytes_from_hex(hex)?;
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_array(self) -> [u8; Self::length()] {
        self.0
    }

    pub fn as_hash(&self) -> Hash {
        Hash::from_array(self.0)
    }
}

impl TryFrom<&[u8]> for RistrettoPublicKeyBytes {
    type Error = InvalidByteLengthError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(value)
    }
}

impl AsRef<[u8]> for RistrettoPublicKeyBytes {
    fn as_ref(&self) -> &[u8] {
        self.deref().as_ref()
    }
}

impl From<[u8; 32]> for RistrettoPublicKeyBytes {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl Display for RistrettoPublicKeyBytes {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_hash())
    }
}

impl Deref for RistrettoPublicKeyBytes {
    type Target = [u8; 32];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromStr for RistrettoPublicKeyBytes {
    type Err = HashParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = fixed_bytes_from_hex(s)?;
        Ok(Self(bytes))
    }
}

// NB: match the implementation used in tari_crypto
#[cfg(feature = "borsh")]
impl borsh::BorshSerialize for RistrettoPublicKeyBytes {
    fn serialize<W: borsh::io::Write>(&self, writer: &mut W) -> borsh::io::Result<()> {
        borsh::BorshSerialize::serialize(&self.as_bytes(), writer)
    }
}

#[cfg(feature = "borsh")]
impl borsh::BorshDeserialize for RistrettoPublicKeyBytes {
    fn deserialize_reader<R>(reader: &mut R) -> Result<Self, borsh::io::Error>
    where R: borsh::io::Read {
        let bytes = borsh::BorshDeserialize::deserialize_reader(reader)?;
        Ok(Self(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize)]
    struct TestCase {
        a: u32,
        bytes: RistrettoPublicKeyBytes,
    }

    #[test]
    fn serialize_deserialize_bor() {
        let encode = tari_bor::encode(&TestCase {
            a: 123,
            bytes: RistrettoPublicKeyBytes::from([1u8; 32]),
        })
        .unwrap();
        let decode = tari_bor::decode::<TestCase>(&encode).unwrap();
        assert_eq!(
            RistrettoPublicKeyBytes::from([1u8; 32]),
            decode.bytes,
            "Failed to serialize/deserialize RistrettoPublicKeyBytes",
        );
        assert_eq!(123, decode.a, "Failed to serialize/deserialize RistrettoPublicKeyBytes",);
    }
}
