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

    let row_limit = req.limit.unwrap_or(1_000);
    let row_skip = req.page.unwrap_or(0).saturating_mul(row_limit);
    let mut skipped = 0usize;
    let mut emitted = 0usize;
    let mut count = 0usize;
    for result in iter {
        let ((shard, state_version), data) = result?;
        let encoded_key = cf.encode_key(&(shard, state_version));
        let key_hex = hex::encode(&encoded_key);
        for (i, transition) in data.transitions.into_iter().enumerate() {
            if skipped < row_skip {
                skipped += 1;
                continue;
            }
            if emitted < row_limit {
                let substate = substate_cf.get(&transition.substate_address, OPERATION).optional()?;
                table.add_row(json!({
                    "id": format!("{}-{}", key_hex, i),
                    "epoch": data.epoch,
                    "shard": shard,
                    "state_version": state_version,
                    "substate_id": substate.as_ref().map(|s| s.substate_id()),
                    "version": substate.as_ref().map(|s| s.version()),
                    "transition": transition.transition,
                }));
                emitted += 1;
            }

            count += 1;
        }
    }
    table.set_total_entries(count);

    Ok(Json(table))
}
