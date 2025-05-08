//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{Extension, Json};
use serde::Serialize;

use crate::{
    config::DatabaseConfig,
    webserver::{context::HandlerContext, error::WebError},
};

#[derive(Debug, Clone, Serialize)]
pub struct ListDatabasesResponse {
    pub databases: Vec<DatabaseConfig>,
}

pub async fn list(Extension(context): Extension<Arc<HandlerContext>>) -> Result<Json<ListDatabasesResponse>, WebError> {
    Ok(Json(ListDatabasesResponse {
        databases: context.config().dbs.clone(),
    }))
}
