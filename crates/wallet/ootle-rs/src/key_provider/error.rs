//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[derive(Debug, thiserror::Error)]
pub enum KeyProviderError {
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl KeyProviderError {
    pub fn other<E>(err: E) -> Self
    where E: std::error::Error + Send + Sync + 'static {
        KeyProviderError::Other(Box::new(err))
    }
}
