//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{extract::Path, Extension, Json};
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

#[derive(Debug, Clone, Serialize)]
pub struct ListColumnFamiliesResponse {
    pub cfs: Vec<ColumnFamilyInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnFamilyInfo {
    pub name: String,
    pub num_entries: usize,
    pub total_entries_bytes: usize,
}

pub async fn list_column_families(
    Extension(context): Extension<Arc<HandlerContext>>,
    Path(db_name): Path<String>,
) -> Result<Json<ListColumnFamiliesResponse>, WebError> {
    let db = context.open_db(&db_name)?;

    let cf_info = db.column_family_info()?;

    Ok(Json(ListColumnFamiliesResponse {
        cfs: cf_info
            .into_iter()
            .map(|cf| ColumnFamilyInfo {
                name: cf.name,
                num_entries: cf.num_entries,
                total_entries_bytes: cf.total_entries_bytes,
            })
            .collect(),
    }))
}
