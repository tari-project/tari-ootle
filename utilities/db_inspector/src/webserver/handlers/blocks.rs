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
    handlers::types::{decode_hex_prefix, Column, TableRequest, TableResponse},
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
        Column::new("shard_group", "ShardGroup"),
        Column::new("num_commands", "#Cmds"),
        Column::new("commands", "Commands"),
        Column::new("proposed_by", "Proposed by"),
        Column::new("parent", "Parent"),
        Column::new("state_hash", "State Hash"),
        Column::new("epoch_hash", "Epoch Hash"),
    ]);
    let tx = db.read_only_context();

    let cf = tx.cf(models::block::BlockModel)?;
    let query_cf = tx.cf(models::block::ByEpochQuery)?;
    let ordering = if req.asc {
        Ordering::Ascending
    } else {
        Ordering::Descending
    };
    let maybe_prefix = req.query_prefix_hex.as_ref().and_then(|s| decode_hex_prefix(s).ok());

    let page_size = req.limit.unwrap_or(1_000);
    let skip = req.page.unwrap_or(0) * page_size;
    let iter = query_cf.query_end_range_key_iterator(ordering, &Epoch::max());
    let mut num_returned = 0;
    for result in iter.skip(skip) {
        let (_, _, block_id) = result?;
        if maybe_prefix
            .as_ref()
            .is_some_and(|bytes| !block_id.as_bytes().starts_with(bytes))
        {
            continue;
        }
        num_returned += 1;
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
            "shard_group": format!("{}-{}", block.shard_group().start().as_u32(), block.shard_group().end().as_u32()),
            "num_commands": block.commands().len(),
            "commands": block.commands(),
            "proposed_by": block.proposed_by(),
            "parent": block.header().parent(),
            "state_hash": hex::encode(block.header().state_merkle_root()),
            "epoch_hash": hex::encode(block.header().epoch_hash()),
        }));
        if num_returned == page_size {
            break;
        }
    }
    let total = cf.count(OPERATION)?;
    table.set_total_entries(total);

    Ok(Json(table))
}
