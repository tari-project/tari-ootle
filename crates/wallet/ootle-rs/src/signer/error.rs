//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub type Result<T> = std::result::Result<T, SignerError>;

#[derive(Debug, thiserror::Error)]
pub enum SignerError {
    #[error("Invalid credentials: {0}")]
    InvalidCredentials(String),
    #[error("Signature error: {0}")]
    SignatureError(#[from] signature::Error),
    #[error("Other signer error: {0}")]
    Other(String),
}

impl SignerError {
    pub fn other<S: Into<String>>(msg: S) -> Self {
        Self::Other(msg.into())
    }
}
