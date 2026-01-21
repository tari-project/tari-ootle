//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub type Result<T> = std::result::Result<T, SignerError>;

#[derive(Debug, thiserror::Error)]
pub enum SignerError {
    #[error("Invalid credentials: {0}")]
    InvalidCredentials(String),
}
