//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[derive(Debug, thiserror::Error)]
pub enum OotleWasmError {
    #[error("JSON deserialization failed: {0}")]
    JsonDeserialize(#[from] serde_json::Error),
    #[error("BOR encoding failed: {0}")]
    BorEncode(#[from] tari_bor::BorError),
    #[error("Invalid secret key: {0}")]
    InvalidSecretKey(String),
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("Signing failed: {0}")]
    SigningFailed(String),
    #[error("Invalid network: {0}")]
    InvalidNetwork(String),
    #[error("Invalid pay reference: length {0} (must be 1-64 bytes)")]
    InvalidPayRef(usize),
    #[error("Invalid address: {0}")]
    InvalidAddress(String),
}
