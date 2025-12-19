//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{
    extract::{Path, Query},
    Extension,
    Json,
};
use serde_json::json;
use tari_ootle_storage::Ordering;
use tari_state_store_rocksdb::column_families;

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
    const OPERATION: &str = "list_foreign_substate_pledges";
    let db = context.open_db(&db_name)?;
    let mut table = TableResponse::new([
        Column::new("tx_id", "Transaction ID"),
        Column::new("substate_id", "Substate ID"),
        Column::new("value", "Value"),
        Column::new("lock_type", "Lock Type"),
    ]);
    let tx = db.read_only_context();

    let cf = tx.cf(column_families::foreign_substate_pledge::ForeignSubstatePledgeCf)?;

    let ordering = if req.desc {
        Ordering::Descending
    } else {
        Ordering::Ascending
    };
    let iter = if let Some(prefix_hex) = req.query.as_ref() {
        let key_prefix =
            decode_hex_prefix::<column_families::foreign_substate_pledge::ForeignSubstatePledgeCf>(prefix_hex)?;
        cf.range_iterator(ordering, key_prefix.as_slice()..)
    } else {
        let empty = Vec::<u8>::new();
        cf.range_iterator(ordering, empty.as_slice()..)
    };

    let page_size = req.limit.unwrap_or(1_000);
    let skip = req.page.unwrap_or(0) * page_size;
    for result in iter.skip(skip).take(page_size) {
        let ((tx_id, substate_addr), data) = result?;
        let encoded_key = cf.encode_key(&(tx_id, substate_addr));
        table.add_row(json!({
            "id": hex::encode(encoded_key),
            "tx_id": tx_id,
            "substate_id": data.versioned_substate_id(),
            "value": data.substate_value(),
            "lock_type": data.as_substate_lock_type(),
        }));
    }
    let total = cf.count(OPERATION)?;
    table.set_total_entries(total);

    Ok(Json(table))
}
