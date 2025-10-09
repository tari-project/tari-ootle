//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use reqwest::StatusCode;
use tari_ootle_common_types::optional::IsNotFoundError;

#[derive(Debug, thiserror::Error)]
pub enum IndexerJrpcClientError {
    #[error("Failed to deserialize response for method {method}: {source} - response: {response}")]
    DeserializeResponse {
        source: serde_json::Error,
        method: &'static str,
        response: serde_json::Value,
    },
    #[error("Failed to serialize request for method {method}: {source}")]
    SerializeRequest { method: String, source: serde_json::Error },
    #[error("Failed to send request: {source}")]
    RequestFailed {
        #[from]
        source: reqwest::Error,
    },
    #[error("Request failed: code: {code} message: {message}")]
    RequestFailedWithStatus { code: i64, message: String },
    #[error("Invalid response: {message}")]
    InvalidResponse { message: String },
}

impl IsNotFoundError for IndexerJrpcClientError {
    fn is_not_found_error(&self) -> bool {
        match self {
            Self::RequestFailedWithStatus { code, .. } => *code == 404,
            Self::RequestFailed { source } => source.status().map(|s| s == StatusCode::NOT_FOUND).unwrap_or(false),
            _ => false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IndexerRestClientError {
    #[error("Failed to deserialize response for path {path}: {source}")]
    DeserializeResponse { source: anyhow::Error, path: String },
    #[error("Failed to serialize request for path {path}: {source}")]
    SerializeRequest { path: String, source: anyhow::Error },
    #[error("Failed to send request: {source}")]
    RequestFailed {
        #[from]
        source: reqwest::Error,
    },
    #[error("Server returned an error: {source} details: {}", .details.as_deref().unwrap_or("none"))]
    ErrorResponse {
        source: reqwest::Error,
        details: Option<String>,
    },
    #[error("Request failed: code: {code} message: {message}")]
    RequestFailedWithStatus { code: i64, message: String },
    #[error("Invalid response: {message}")]
    InvalidResponse { message: String },
}

impl IsNotFoundError for IndexerRestClientError {
    fn is_not_found_error(&self) -> bool {
        match self {
            Self::RequestFailedWithStatus { code, .. } => *code == 404,
            Self::RequestFailed { source, .. } | Self::ErrorResponse { source, .. } => {
                source.status().map(|s| s == StatusCode::NOT_FOUND).unwrap_or(false)
            },
            _ => false,
        }
    }
}
