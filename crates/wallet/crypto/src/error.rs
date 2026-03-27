//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use chacha20poly1305::aead;
use tari_crypto::errors::RangeProofError;

#[derive(Debug, thiserror::Error)]
pub enum StealthProofError {
    #[error("Range proof error: {0}")]
    RangeProof(RangeProofError),
    #[error("Aead error")]
    AeadError,
    #[error("Negative amount")]
    NegativeAmount,
}

impl From<aead::Error> for StealthProofError {
    fn from(_value: aead::Error) -> Self {
        Self::AeadError
    }
}

impl From<RangeProofError> for StealthProofError {
    fn from(value: RangeProofError) -> Self {
        Self::RangeProof(value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WalletCryptoError {
    #[error("Stealth proof error: {0}")]
    StealthProofError(#[from] StealthProofError),
    #[error("Failed to decrypt data: {details}")]
    FailedDecryptData { details: String },
    #[error("Failed to encrypt data: {details}")]
    FailedEncryptData { details: String },
    #[error("Commitment does not match commitment derived from decrypted data")]
    CommitmentMismatchDecryptedData,
    #[error("Invalid argument {name}: {details}")]
    InvalidArgument { name: &'static str, details: String },
    #[error("AEAD error: {0}")]
    AeadError(aead::Error),
    #[error("BUG: Invariant violated: {details}")]
    Invariant { details: String },
}

impl From<aead::Error> for WalletCryptoError {
    fn from(err: aead::Error) -> Self {
        WalletCryptoError::AeadError(err)
    }
}
