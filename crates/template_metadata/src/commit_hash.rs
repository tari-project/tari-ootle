//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use serde::{Deserialize, Serialize};

const SHA1_LEN: usize = 20;

/// A git commit hash identifying the source a template was built from.
///
/// The serde representation (`{"Sha1": [u8; 20]}`) is part of the wallet daemon's JSON API and the TypeScript
/// bindings, so new hash kinds are added as new variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CommitHash {
    Sha1([u8; SHA1_LEN]),
}

impl CommitHash {
    /// Parse a hex-encoded commit hash. Upper- and lowercase digits are accepted.
    pub fn from_hex(s: &str) -> Result<Self, CommitHashError> {
        let mut buf = [0u8; SHA1_LEN];
        hex::decode_to_slice(s, &mut buf).map_err(CommitHashError::InvalidHex)?;
        Ok(Self::Sha1(buf))
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Sha1(bytes) => bytes,
        }
    }
}

impl TryFrom<&[u8]> for CommitHash {
    type Error = CommitHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let sha1 = bytes
            .try_into()
            .map_err(|_| CommitHashError::InvalidLength(bytes.len()))?;
        Ok(Self::Sha1(sha1))
    }
}

impl fmt::Display for CommitHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.as_bytes()))
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CommitHashError {
    #[error("commit hash must be {hex_len} hex characters: {0}", hex_len = SHA1_LEN * 2)]
    InvalidHex(hex::FromHexError),
    #[error("commit hash must be {SHA1_LEN} bytes, got {0}")]
    InvalidLength(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_hex_rejects_wrong_length_and_non_hex() {
        assert!(matches!(
            CommitHash::from_hex("0123456789abcdef"),
            Err(CommitHashError::InvalidHex(_))
        ));
        assert!(matches!(
            CommitHash::from_hex("zz23456789abcdeffedcba9876543210deadbeef"),
            Err(CommitHashError::InvalidHex(_))
        ));
    }

    #[test]
    fn try_from_rejects_wrong_length() {
        assert_eq!(
            CommitHash::try_from([0u8; 32].as_slice()),
            Err(CommitHashError::InvalidLength(32))
        );
        assert_eq!(
            CommitHash::try_from([7u8; 20].as_slice()),
            Ok(CommitHash::Sha1([7u8; 20]))
        );
    }
}
