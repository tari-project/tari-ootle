//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod block_diff;
pub mod block_diff_substate_id_index;
pub mod blocks;
pub mod bookkeeping;
pub mod column_families;
pub mod databases;
pub mod foreign_substate_pledges;
pub mod state_transitions;
pub mod tables;
mod types;

use crate::webserver::error::WebError;

pub async fn not_found() -> Result<impl axum::response::IntoResponse, WebError> {
    Ok(WebError::not_found("Endpoint not found"))
}

pub fn slugify_type_name<T>(_cf: T) -> String {
    let type_name = std::any::type_name::<T>();
    type_name.rsplit("::").take(2).collect::<Vec<_>>().join("--")
}
