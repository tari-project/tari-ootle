//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use chacha20poly1305::aead;
use tari_crypto::errors::RangeProofError;

#[derive(Debug, thiserror::Error)]
pub enum ConfidentialProofError {
    #[error("Range proof error: {0}")]
    RangeProof(RangeProofError),
    #[error("Aead error")]
    AeadError,
    #[error("Negative amount")]
    NegativeAmount,
}

impl From<aead::Error> for ConfidentialProofError {
    fn from(_value: aead::Error) -> Self {
        Self::AeadError
    }
}

impl From<RangeProofError> for ConfidentialProofError {
    fn from(value: RangeProofError) -> Self {
        Self::RangeProof(value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WalletCryptoError {
    #[error("Confidential proof error: {0}")]
    ConfidentialProof(#[from] ConfidentialProofError),
    #[error("Failed to decrypt data: {details}")]
    FailedDecryptData { details: String },
    #[error("Unable to open the commitment")]
    UnableToOpenCommitment,
    #[error("Invalid argument {name}: {details}")]
    InvalidArgument { name: &'static str, details: String },
    #[error("AEAD error: {0}")]
    AeadError(aead::Error),
}

impl From<aead::Error> for WalletCryptoError {
    fn from(err: aead::Error) -> Self {
        WalletCryptoError::AeadError(err)
    }
}
