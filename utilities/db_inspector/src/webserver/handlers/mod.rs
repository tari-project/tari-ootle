//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod blocks;
pub mod bookkeeping;
pub mod databases;
pub mod tables;
mod types;

use crate::webserver::error::WebError;

pub async fn not_found() -> Result<impl axum::response::IntoResponse, WebError> {
    Ok(WebError::not_found("Endpoint not found"))
}
