//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use log::error;
use serde::Serialize;
use tari_state_store_rocksdb::error::RocksDbStorageError;

#[derive(Debug, Clone, Serialize)]
pub struct WebError {
    pub message: String,
    pub code: u16,
}

impl WebError {
    pub fn bad_request<T: Into<String>>(message: T) -> Self {
        Self {
            message: message.into(),
            code: StatusCode::BAD_REQUEST.as_u16(),
        }
    }

    pub fn not_found<T: Into<String>>(message: T) -> Self {
        Self {
            message: message.into(),
            code: StatusCode::NOT_FOUND.as_u16(),
        }
    }

    pub fn internal_server_error<T: ToString>(message: T) -> Self {
        Self {
            message: message.to_string(),
            code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        error!("WebError({}): {}", self.code, self.message);

        (
            StatusCode::from_u16(self.code).expect("invalid status code"),
            Json(self),
        )
            .into_response()
    }
}

impl From<anyhow::Error> for WebError {
    fn from(err: anyhow::Error) -> Self {
        Self::internal_server_error(err)
    }
}

impl From<RocksDbStorageError> for WebError {
    fn from(err: RocksDbStorageError) -> Self {
        Self::internal_server_error(err)
    }
}
