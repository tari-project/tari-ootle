//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{
    extract::{Path, Query},
    Extension,
    Json,
};
use serde_json::json;
use tari_ootle_common_types::optional::Optional;
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
    const OPERATION: &str = "list_state_transitions";
    let db = context.open_db(&db_name)?;
    let mut table = TableResponse::new([
        Column::new("epoch", "Epoch"),
        Column::new("shard", "Shard"),
        Column::new("state_version", "State Version"),
        Column::new("substate_id", "Substate ID"),
        Column::new("version", "Version"),
        Column::new("transition", "Transition"),
    ]);
    let tx = db.read_only_context();

    let cf = tx.cf(column_families::state_transition::StateTransitionCf)?;

    let substate_cf = tx.cf(column_families::substate::SubstateCf)?;
    let ordering = if req.asc {
        Ordering::Ascending
    } else {
        Ordering::Descending
    };
    let iter = if let Some(prefix_hex) = req.query.as_ref() {
        let key_prefix = decode_hex_prefix(prefix_hex)?;
        cf.range_iterator(ordering, key_prefix.as_slice()..)
    } else {
        let empty = Vec::<u8>::new();
        cf.range_iterator(ordering, empty.as_slice()..)
    };

    let page_size = req.limit.unwrap_or(1_000);
    let skip = req.page.unwrap_or(0) * page_size;
    for result in iter.skip(skip).take(page_size) {
        let ((shard, state_version), data) = result?;
        let encoded_key = cf.encode_key(&(shard, state_version));
        for (i, transition) in data.transitions.into_iter().enumerate() {
            let substate = substate_cf.get(&transition.substate_address, OPERATION).optional()?;
            table.add_row(json!({
                "id": hex::encode(&encoded_key) + &format!("-{}", i),
                "epoch": data.epoch,
                "shard": shard,
                "state_version": state_version,
                "substate_id": substate.as_ref().map(|s| s.substate_id()),
                "version": substate.as_ref().map(|s| s.version()),
                "transition": transition.transition,
            }));
        }
    }
    let total = cf.count(OPERATION)?;
    table.set_total_entries(total);

    Ok(Json(table))
}
