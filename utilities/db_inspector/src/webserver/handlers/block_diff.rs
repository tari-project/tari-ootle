//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{
    extract::{Path, Query},
    Extension,
    Json,
};
use serde_json::json;
use tari_dan_storage::Ordering;
use tari_state_store_rocksdb::{column_families as cfs, error::RocksDbStorageError, traits::Cf};

use crate::webserver::{
    context::HandlerContext,
    error::WebError,
    handlers::types::{decode_hex_prefix, Column, TableRequest, TableResponse},
};

pub async fn list(
    Extension(context): Extension<Arc<HandlerContext>>,
    Path(db_name): Path<String>,
    Query(req): Query<TableRequest>,
) -> Result<Json<TableResponse>, WebError> {
    const OPERATION: &str = "list_state_transitions";
    let db = context.open_db(&db_name)?;
    let mut table = TableResponse::new([
        Column::new("block_id", "Block Id"),
        Column::new("shard", "Shard"),
        Column::new("substate_id", "Substate ID"),
        Column::new("version", "Version"),
        Column::new("substate", "Substate"),
    ]);
    let tx = db.read_only_context();

    let cf = tx.cf(cfs::block_diff::BlockDiffCf)?;
    let ordering = if req.asc {
        Ordering::Ascending
    } else {
        Ordering::Descending
    };
    type Key = <cfs::block_diff::BlockDiffCf as Cf>::Key;
    type Value = <cfs::block_diff::BlockDiffCf as Cf>::Value;
    let iter: Box<dyn Iterator<Item = Result<(Key, Value), RocksDbStorageError>>> =
        if let Some(prefix_hex) = req.query_prefix_hex.as_ref() {
            let key_prefix = decode_hex_prefix(prefix_hex)?;
            Box::new(cf.prefix_range_iterator_raw_key(ordering, key_prefix))
        } else {
            Box::new(cf.iterator(ordering, OPERATION))
        };

    let page_size = req.limit.unwrap_or(1_000);
    let skip = req.page.unwrap_or(0) * page_size;
    for result in iter.skip(skip).take(page_size) {
        let (id, data) = result?;
        let encoded_key = cf.encode_key(&id);
        table.add_row(json!({
            "id": hex::encode(encoded_key),
            "block_id": id.block_id,
            "substate_id": id.substate_id,
            "version": id.version,
            "shard": data.shard(),
            "substate": data.substate(),
        }));
    }
    let total = cf.count(OPERATION)?;
    table.set_total_entries(total);

    Ok(Json(table))
}
