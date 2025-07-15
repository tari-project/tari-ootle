//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_abi::rust::{
    fmt::{Display, Formatter},
    ops::Deref,
};

use crate::{
    crypto::{InvalidByteLengthError, RistrettoPublicKeyBytes},
    hex::fixed_bytes_from_hex,
    serde_helpers,
    Hash,
    HashParseError,
};

/// A Pedersen Commitment byte contents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize, borsh::BorshDeserialize))]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct PedersenCommitmentBytes(
    #[serde(with = "serde_helpers::fixed_hex")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    [u8; PedersenCommitmentBytes::length()],
);

impl PedersenCommitmentBytes {
    pub const fn length() -> usize {
        32
    }

    pub const fn zero() -> Self {
        Self([0u8; Self::length()])
    }

    pub fn from_public_key(commitment: RistrettoPublicKeyBytes) -> Self {
        Self(commitment.into_array())
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
        Ok(PedersenCommitmentBytes(key))
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

impl TryFrom<&[u8]> for PedersenCommitmentBytes {
    type Error = InvalidByteLengthError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(value)
    }
}

impl AsRef<[u8]> for PedersenCommitmentBytes {
    fn as_ref(&self) -> &[u8] {
        self.deref().as_ref()
    }
}

impl From<[u8; 32]> for PedersenCommitmentBytes {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl Display for PedersenCommitmentBytes {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_hash())
    }
}

impl Deref for PedersenCommitmentBytes {
    type Target = [u8; 32];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
