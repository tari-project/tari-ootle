//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod blocks;
pub mod bookkeeping;
pub mod databases;
pub mod tables;
mod types;

use tari_dan_common_types::PeerAddress;
use tari_state_store_rocksdb::RocksDbReadOnlyStateStore;

use crate::{
    helpers::open_db,
    webserver::{context::HandlerContext, error::WebError},
};

pub async fn not_found() -> Result<impl axum::response::IntoResponse, WebError> {
    Ok(WebError::not_found("Endpoint not found"))
}

pub fn web_open_db(
    context: &HandlerContext,
    name: Option<&str>,
) -> Result<RocksDbReadOnlyStateStore<PeerAddress>, WebError> {
    let config = context
        .config()
        .get_database(name)
        .ok_or_else(|| WebError::bad_request(format!("Database {} not found", name.unwrap_or("default"))))?;
    let db = open_db(config).map_err(|e| {
        WebError::internal_server_error(format!("Failed to open database {}: {}", name.unwrap_or("default"), e))
    })?;
    Ok(db)
}
