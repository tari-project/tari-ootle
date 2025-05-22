//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{
    extract::{Path, Query},
    Extension,
    Json,
};
use serde_json::json;
use tari_dan_common_types::optional::Optional;
use tari_dan_storage::Ordering;
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
    const OPERATION: &str = "list_state_transitions";
    let db = context.open_db(&db_name)?;
    let mut table = TableResponse::new([
        Column::new("epoch", "Epoch"),
        Column::new("shard", "Shard"),
        Column::new("seq", "Seq"),
        Column::new("substate_id", "Substate ID"),
        Column::new("version", "Version"),
        Column::new("transition", "Transition"),
        Column::new("substate_address", "Substate Address"),
    ]);
    let tx = db.read_only_context();

    let cf = tx.cf(column_families::state_transition::StateTransitionCf)?;

    let substate_cf = tx.cf(column_families::substate::SubstateCf)?;
    let ordering = if req.asc {
        Ordering::Ascending
    } else {
        Ordering::Descending
    };
    let iter = if let Some(prefix_hex) = req.query_prefix_hex.as_ref() {
        let key_prefix = decode_hex_prefix(prefix_hex)?;
        cf.range_iterator(ordering, key_prefix.as_slice()..)
    } else {
        let empty = Vec::<u8>::new();
        cf.range_iterator(ordering, empty.as_slice()..)
    };

    let page_size = req.limit.unwrap_or(1_000);
    let skip = req.page.unwrap_or(0) * page_size;
    for result in iter.skip(skip).take(page_size) {
        let (id, data) = result?;
        let encoded_key = cf.encode_key(&id);
        let substate = substate_cf.get(&data.substate_address, OPERATION).optional()?;
        table.add_row(json!({
            "id": hex::encode(encoded_key),
            "epoch": id.epoch(),
            "shard": id.shard(),
            "seq": id.seq(),
            "substate_id": substate.as_ref().map(|s| s.substate_id()),
            "version": substate.as_ref().map(|s| s.version()),
            "transition": data.transition,
            "substate_address": data.substate_address,
        }));
    }
    let total = cf.count(OPERATION)?;
    table.set_total_entries(total);

    Ok(Json(table))
}
