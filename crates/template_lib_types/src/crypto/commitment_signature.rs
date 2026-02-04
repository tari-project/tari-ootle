//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
use serde::{Deserialize, Serialize};

use crate::crypto::{InvalidByteLengthError, PedersenCommitmentBytes, scalar::Scalar32Bytes};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CommitmentSignatureBytes {
    public_nonce: PedersenCommitmentBytes,
    u: Scalar32Bytes,
    v: Scalar32Bytes,
}

impl CommitmentSignatureBytes {
    pub const fn length() -> usize {
        PedersenCommitmentBytes::length() + Scalar32Bytes::length() + Scalar32Bytes::length()
    }

    pub fn zero() -> Self {
        Self {
            public_nonce: PedersenCommitmentBytes::zero(),
            u: Scalar32Bytes::zero(),
            v: Scalar32Bytes::zero(),
        }
    }

    pub const fn new(public_nonce: PedersenCommitmentBytes, u: Scalar32Bytes, v: Scalar32Bytes) -> Self {
        Self { public_nonce, u, v }
    }

    pub fn try_from_parts(public_nonce: &[u8], u: &[u8], v: &[u8]) -> Result<Self, InvalidByteLengthError> {
        let public_nonce = PedersenCommitmentBytes::from_bytes(public_nonce)?;
        let u = Scalar32Bytes::from_bytes(u)?;
        let v = Scalar32Bytes::from_bytes(v)?;
        Ok(Self::new(public_nonce, u, v))
    }

    pub fn from_bytes(mut bytes: &[u8]) -> Result<Self, InvalidByteLengthError> {
        if bytes.len() != Self::length() {
            return Err(InvalidByteLengthError {
                size: bytes.len(),
                expected: Self::length(),
            });
        }

        let reader = &mut bytes;
        // Panic: Length checked before
        let public_nonce = read_n_bytes(reader, PedersenCommitmentBytes::length());
        let u = read_n_bytes(reader, Scalar32Bytes::length());
        let v = read_n_bytes(reader, Scalar32Bytes::length());

        Self::try_from_parts(public_nonce, u, v)
    }

    pub fn public_nonce(&self) -> &PedersenCommitmentBytes {
        &self.public_nonce
    }

    pub fn u(&self) -> &Scalar32Bytes {
        &self.u
    }

    pub fn v(&self) -> &Scalar32Bytes {
        &self.v
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::length());
        bytes.extend_from_slice(self.public_nonce.as_bytes());
        bytes.extend_from_slice(self.u.as_bytes());
        bytes.extend_from_slice(self.v.as_bytes());
        bytes
    }
}

impl TryFrom<&[u8]> for CommitmentSignatureBytes {
    type Error = InvalidByteLengthError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(value)
    }
}

/// Reads `length` bytes from `bytes` and returns a slice of those bytes.
/// Panics if `bytes` is shorter than `length`.
fn read_n_bytes<'a>(bytes: &mut &'a [u8], length: usize) -> &'a [u8] {
    let slice = bytes.get(..length).expect("read_n_bytes: Not enough bytes available");
    *bytes = bytes
        .get(length..)
        .expect("read_n_bytes: if reading succeeds, len.. bound must be valid");
    slice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_and_from_bytes() {
        let public_nonce = PedersenCommitmentBytes::from([1u8; 32]);
        let u = Scalar32Bytes::from([2u8; 32]);
        let v = Scalar32Bytes::from([3u8; 32]);
        let signature_bytes = CommitmentSignatureBytes::new(public_nonce, u, v);

        let bytes = signature_bytes.to_bytes();
        let deserialized_signature_bytes = CommitmentSignatureBytes::from_bytes(&bytes).unwrap();

        assert_eq!(signature_bytes, deserialized_signature_bytes);
    }
}
