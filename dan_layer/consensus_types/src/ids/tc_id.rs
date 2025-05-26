//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};
use tari_common_types::types::{FixedHash, FixedHashSizeError};
use tari_engine_types::serde_with;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, BorshSerialize)]
#[serde(transparent)]
pub struct TcId(#[serde(with = "serde_with::hex")] FixedHash);

impl TcId {
    /// Represents a zero/null QC. This QC is used to represent the unsigned initial QC.
    pub const fn zero() -> Self {
        Self(FixedHash::zero())
    }

    pub fn new<T: Into<FixedHash>>(hash: T) -> Self {
        Self(hash.into())
    }

    pub const fn hash(&self) -> &FixedHash {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|b| *b == 0)
    }
}

impl AsRef<[u8]> for TcId {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl From<FixedHash> for TcId {
    fn from(value: FixedHash) -> Self {
        Self(value)
    }
}

impl From<[u8; 32]> for TcId {
    fn from(value: [u8; 32]) -> Self {
        Self(value.into())
    }
}

impl TryFrom<&[u8]> for TcId {
    type Error = FixedHashSizeError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        FixedHash::try_from(value).map(Self)
    }
}

impl Display for TcId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl AsRef<TcId> for TcId {
    fn as_ref(&self) -> &TcId {
        self
    }
}
