//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{
    Extension,
    Json,
    extract::{Path, Query},
};
use serde_json::json;
use tari_ootle_storage::Ordering;
use tari_state_store_rocksdb::{column_families as cfs, error::RocksDbStorageError, traits::Cf};

use crate::webserver::{
    context::HandlerContext,
    error::WebError,
    handlers::types::{Column, TableRequest, TableResponse, decode_hex_prefix},
};

pub async fn list(
    Extension(context): Extension<Arc<HandlerContext>>,
    Path(db_name): Path<String>,
    Query(req): Query<TableRequest>,
) -> Result<Json<TableResponse>, WebError> {
    const OPERATION: &str = "list_block_diff";
    let db = context.open_db(&db_name)?;
    let mut table = TableResponse::new([
        Column::new("block_id", "Block Id"),
        Column::new("shard", "Shard"),
        Column::new("substate_id", "Substate ID"),
        Column::new("version", "Version"),
        Column::new("substate", "Substate"),
    ]);
    let tx = db.read_only_context();

    let cf = tx.cf(cfs::block_diff::BlockDiffRecordCf)?;
    let ordering = if req.desc {
        Ordering::Descending
    } else {
        Ordering::Ascending
    };
    type Key = <cfs::block_diff::BlockDiffRecordCf as Cf>::Key;
    type Value = <cfs::block_diff::BlockDiffRecordCf as Cf>::Value;
    let iter: Box<dyn Iterator<Item = Result<(Key, Value), RocksDbStorageError>>> =
        if let Some(prefix_hex) = req.query.as_ref() {
            let key_prefix = decode_hex_prefix::<cfs::block_diff::BlockDiffRecordCf>(prefix_hex)?;
            Box::new(cf.prefix_range_iterator_raw_key(ordering, key_prefix))
        } else {
            Box::new(cf.iterator(ordering, OPERATION))
        };

    // Each record holds one block's changes, and the table pages over the changes.
    let page_size = req.limit.unwrap_or(1_000);
    let skip = req.page.unwrap_or(0).saturating_mul(page_size);
    let mut total = 0usize;
    for result in iter {
        let (block_id, changes) = result?;
        let encoded_key = hex::encode(cf.encode_key(&block_id));
        for change in changes {
            total += 1;
            if total <= skip || total > skip.saturating_add(page_size) {
                continue;
            }
            let versioned = change.versioned_substate_id();
            table.add_row(json!({
                "id": encoded_key,
                "block_id": block_id,
                "substate_id": versioned.substate_id(),
                "version": versioned.version(),
                "shard": change.shard(),
                "substate": change.substate(),
            }));
        }
    }
    table.set_total_entries(total);

    Ok(Json(table))
}
