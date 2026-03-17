//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[derive(Debug, thiserror::Error)]
pub enum OotleWasmError {
    #[error("JSON deserialization failed: {0}")]
    JsonDeserialize(#[from] serde_json::Error),
    #[error("BOR encoding failed: {0}")]
    BorEncode(#[from] tari_bor::BorError),
    #[error("Hex decoding failed: {0}")]
    HexDecode(#[from] hex::FromHexError),
    #[error("Invalid secret key: {0}")]
    InvalidSecretKey(String),
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("Signing failed: {0}")]
    SigningFailed(String),
}
