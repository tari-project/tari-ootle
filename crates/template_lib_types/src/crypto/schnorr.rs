//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::{fmt, prelude::*};

use crate::crypto::{InvalidByteLengthError, RistrettoPublicKeyBytes, scalar::Scalar32Bytes};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SchnorrSignatureBytes {
    #[n(0)]
    public_nonce: RistrettoPublicKeyBytes,
    #[n(1)]
    signature: Scalar32Bytes,
}

impl SchnorrSignatureBytes {
    pub const fn length() -> usize {
        RistrettoPublicKeyBytes::length() + Scalar32Bytes::length()
    }

    pub const fn zero() -> Self {
        Self {
            public_nonce: RistrettoPublicKeyBytes::zero(),
            signature: Scalar32Bytes::zero(),
        }
    }

    pub const fn new(public_nonce: RistrettoPublicKeyBytes, signature: Scalar32Bytes) -> Self {
        Self {
            public_nonce,
            signature,
        }
    }

    pub fn try_from_raw_parts(public_nonce: &[u8], signature: &[u8]) -> Result<Self, InvalidByteLengthError> {
        let public_nonce = RistrettoPublicKeyBytes::from_bytes(public_nonce)?;
        let signature = Scalar32Bytes::from_bytes(signature)?;
        Ok(Self::new(public_nonce, signature))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, InvalidByteLengthError> {
        if bytes.len() != Self::length() {
            return Err(InvalidByteLengthError {
                size: bytes.len(),
                expected: Self::length(),
            });
        }

        Self::try_from_raw_parts(
            bytes
                .get(..RistrettoPublicKeyBytes::length())
                .expect("Slice length checked before"),
            bytes
                .get(RistrettoPublicKeyBytes::length()..)
                .expect("Slice length checked before"),
        )
    }

    pub fn public_nonce(&self) -> &RistrettoPublicKeyBytes {
        &self.public_nonce
    }

    pub fn signature(&self) -> &Scalar32Bytes {
        &self.signature
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::length());
        bytes.extend_from_slice(self.public_nonce.as_bytes());
        bytes.extend_from_slice(self.signature.as_bytes());
        bytes
    }
}

impl TryFrom<&[u8]> for SchnorrSignatureBytes {
    type Error = InvalidByteLengthError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(value)
    }
}

impl fmt::Display for SchnorrSignatureBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SchnorrSignatureBytes {{ public_nonce: {}, s: {} }}",
            self.public_nonce, self.signature,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_and_from_bytes() {
        let public_nonce = RistrettoPublicKeyBytes::zero();
        let signature = Scalar32Bytes::zero();
        let signature_bytes = SchnorrSignatureBytes::new(public_nonce, signature);

        let bytes = signature_bytes.to_bytes();
        let deserialized_signature_bytes = SchnorrSignatureBytes::from_bytes(&bytes).unwrap();

        assert_eq!(signature_bytes, deserialized_signature_bytes);
    }
}
