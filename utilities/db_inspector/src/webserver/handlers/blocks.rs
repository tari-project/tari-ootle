//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{
    extract::{Path, Query},
    Extension,
    Json,
};
use serde_json::json;
use tari_dan_common_types::Epoch;
use tari_dan_storage::Ordering;
use tari_state_store_rocksdb::models;

use crate::webserver::{
    context::HandlerContext,
    error::WebError,
    handlers::types::{Column, TableRequest, TableResponse},
};

pub async fn list(
    Extension(context): Extension<Arc<HandlerContext>>,
    Path(db_name): Path<String>,
    Query(req): Query<TableRequest>,
) -> Result<Json<TableResponse>, WebError> {
    const OPERATION: &str = "list_blocks";
    let db = context.open_db(&db_name)?;
    let mut table = TableResponse::new([
        Column::new("block_id", "Block ID"),
        Column::new("flags", "Flags"),
        Column::new("height", "Height"),
        Column::new("epoch", "Epoch"),
        Column::new("num_commands", "#Cmds"),
        Column::new("commands", "Commands"),
        Column::new("proposed_by", "Proposed by"),
        Column::new("state_hash", "State Hash"),
    ]);
    let tx = db.read_only_context();

    let cf = tx.cf(models::block::BlockModel)?;
    let query_cf = tx.cf(models::block::ByEpochQuery)?;
    // let ((epoch, height, block_id), _) = query_cf.query_last(OPERATION)?;
    let iter = query_cf.query_end_range_key_iterator(Ordering::Descending, &Epoch::max());
    for result in iter.take(req.limit.unwrap_or(1_000_000)) {
        let (_, _, block_id) = result?;
        let block = cf.get(&block_id, OPERATION)?;
        let mut flags = Vec::new();
        if block.is_committed() {
            flags.push("C");
        }
        if block.is_justified() {
            flags.push("J");
        }
        if block.is_dummy() {
            flags.push("D");
        }
        let flags = flags.join(", ");
        table.add_row(json!({
            "id": block.id(),
            "block_id": block.id(),
            "flags": flags,
            "height": block.height(),
            "epoch": block.epoch(),
            "num_commands": block.commands().len(),
            "commands": block.commands(),
            "proposed_by": block.proposed_by(),
            "state_hash": hex::encode(block.header().state_merkle_root()),
        }));
    }
    let total = cf.count(OPERATION)?;
    table.set_total_entries(total);

    Ok(Json(table))
}
