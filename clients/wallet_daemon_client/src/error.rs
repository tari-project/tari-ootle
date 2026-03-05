//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::optional::IsNotFoundError;

/// The error code used by the wallet daemon server to indicate an unauthorized request.
const UNAUTHORIZED_ERROR_CODE: i64 = 401;

#[derive(Debug, thiserror::Error)]
pub enum WalletDaemonClientError {
    #[error("Failed to deserialize response for method {method}: {source}")]
    DeserializeResponse { source: serde_json::Error, method: String },
    #[error("Failed to serialize request for method {method}: {source}")]
    SerializeRequest { method: String, source: serde_json::Error },
    #[error("Failed to send request: {source}")]
    RequestFailed {
        #[from]
        source: reqwest::Error,
    },
    #[error("Unauthorized: {message}")]
    Unauthorized { message: String },
    #[error("Request failed: code: {code} message: {message}")]
    RequestFailedWithStatus { code: i64, message: String },
    #[error("Invalid response: {message}")]
    InvalidResponse { message: String },
}

impl WalletDaemonClientError {
    /// Returns `true` if this error represents an unauthorized (401) response.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Unauthorized { .. })
    }

    pub(crate) fn from_error_code(code: i64, message: String) -> Self {
        if code == UNAUTHORIZED_ERROR_CODE {
            Self::Unauthorized { message }
        } else {
            Self::RequestFailedWithStatus { code, message }
        }
    }
}

impl IsNotFoundError for WalletDaemonClientError {
    fn is_not_found_error(&self) -> bool {
        match self {
            Self::RequestFailedWithStatus { code, .. } => *code == 404,
            _ => false,
        }
    }
}
