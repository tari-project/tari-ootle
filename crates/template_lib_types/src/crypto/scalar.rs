//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_abi::rust::{fmt, ops::Deref};

use crate::{crypto::InvalidByteLengthError, serde_helpers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct Scalar32Bytes(
    #[serde(with = "serde_helpers::fixed_hex")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    [u8; Scalar32Bytes::length()],
);

impl Scalar32Bytes {
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
        Ok(Self(key))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_array(self) -> [u8; Self::length()] {
        self.0
    }
}

impl TryFrom<&[u8]> for Scalar32Bytes {
    type Error = InvalidByteLengthError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(value)
    }
}

impl AsRef<[u8]> for Scalar32Bytes {
    fn as_ref(&self) -> &[u8] {
        self.deref().as_ref()
    }
}

impl Deref for Scalar32Bytes {
    type Target = [u8; Self::length()];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<[u8; Scalar32Bytes::length()]> for Scalar32Bytes {
    fn from(bytes: [u8; Scalar32Bytes::length()]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for Scalar32Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for x in self.0 {
            write!(f, "{:02x?}", x)?;
        }
        Ok(())
    }
}
