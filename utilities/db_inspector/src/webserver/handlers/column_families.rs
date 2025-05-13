//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{io, path::Path, sync::Arc};

use axum::{extract::Path as ExtractPath, Extension, Json};
use serde::Serialize;
use tokio::fs;

use crate::webserver::{context::HandlerContext, error::WebError};

#[derive(Debug, Clone, Serialize)]
pub struct ListColumnFamiliesResponse {
    pub cfs: Vec<ColumnFamilyInfo>,
    pub dir_size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnFamilyInfo {
    pub name: String,
    pub num_entries: usize,
    pub total_entries_bytes: usize,
}

async fn dir_size<P: AsRef<Path>>(path: P) -> io::Result<u64> {
    async fn dir_size(mut dir: fs::ReadDir) -> io::Result<u64> {
        let mut acc = 0;
        while let Some(entry) = dir.next_entry().await? {
            let size = match entry.metadata().await? {
                data if data.is_dir() => {
                    let dir = fs::read_dir(entry.path()).await?;
                    Box::pin(dir_size(dir)).await?
                },
                data => data.len(),
            };
            acc += size;
        }
        Ok(acc)
    }

    let dir = fs::read_dir(path).await?;
    dir_size(dir).await
}

pub async fn list(
    Extension(context): Extension<Arc<HandlerContext>>,
    ExtractPath(db_name): ExtractPath<String>,
) -> Result<Json<ListColumnFamiliesResponse>, WebError> {
    let db_path = context
        .config()
        .get_database(&db_name)
        .map(|db| &db.path)
        .ok_or_else(|| WebError::bad_request(format!("Database {} not found", db_name)))?;
    let dir_size = dir_size(db_path).await.map_err(|e| {
        WebError::internal_server_error(format!(
            "Error reading dir {} for db {}: {}",
            db_path.display(),
            db_name,
            e
        ))
    })?;

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
        dir_size,
    }))
}
