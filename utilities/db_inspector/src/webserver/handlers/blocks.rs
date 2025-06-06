//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{
    extract::{Path, Query},
    Extension,
    Json,
};
use serde_json::json;
use tari_ootle_common_types::{optional::Optional, Epoch, NodeHeight};
use tari_ootle_storage::Ordering;
use tari_state_store_rocksdb::{column_families as cfs, error::RocksDbStorageError};

use crate::webserver::{
    context::HandlerContext,
    error::WebError,
    handlers::types::{decode_hex_prefix, Column, TableRequest, TableResponse},
};

#[allow(clippy::too_many_lines)]
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
        Column::new("justify", "Proposal Cert"),
        Column::new("timeout", "Timeout Cert"),
        Column::new("parent", "Parent"),
        Column::new("commit_qc", "Commit QC"),
        Column::new("state_hash", "State Hash"),
        Column::new("epoch_hash", "Epoch Hash"),
        Column::new("proposed_by", "Proposed by"),
    ]);
    let tx = db.read_only_context();

    let cf = tx.cf(cfs::block::BlockCf)?;
    let query_cf = tx.cf(cfs::block::ByEpochQuery)?;
    let height_query_cf = tx.cf(cfs::block::ByEpochHeightQuery)?;
    let ordering = if req.asc {
        Ordering::Ascending
    } else {
        Ordering::Descending
    };

    let locked_cf = tx.cf(cfs::bookkeeping::LockedBlockCf)?;
    let locked_block = locked_cf.get_by_default_key(OPERATION).optional()?;

    let page_size = req.limit.unwrap_or(1_000);
    let skip = req.page.unwrap_or(0) * page_size;

    let iter: Box<dyn Iterator<Item = Result<(_, _), RocksDbStorageError>>> =
        match parse_query(&req.query.unwrap_or_default())? {
            Some(BlockQuery::ByHeight(height)) => {
                let epoch = locked_block.as_ref().map(|b| b.epoch()).unwrap_or_else(Epoch::zero);
                let range = match ordering {
                    Ordering::Ascending => (epoch, height)..(Epoch::max(), NodeHeight::max()),
                    Ordering::Descending => (Epoch::zero(), NodeHeight::zero())..(epoch, height + NodeHeight(1)),
                };
                Box::new(height_query_cf.query_range_key_iterator(ordering, range).map(|r| {
                    r.and_then(|(epoch, height, block_id)| {
                        let block = cf.get(&block_id, OPERATION)?;
                        Ok(((epoch, height, block_id), block))
                    })
                }))
            },
            Some(BlockQuery::ByBlockIdPrefix(key_prefix)) => Box::new(
                cf.prefix_range_iterator_raw_key(ordering, key_prefix)
                    .map(|r| r.map(|(k, v)| ((v.epoch(), v.height(), k), v))),
            ),
            None => Box::new(query_cf.query_end_range_key_iterator(ordering, &Epoch::max()).map(|r| {
                r.and_then(|(epoch, height, block_id)| {
                    let block = cf.get(&block_id, OPERATION)?;
                    Ok(((epoch, height, block_id), block))
                })
            })),
        };

    let mut num_returned = 0;
    for result in iter.skip(skip) {
        let ((_, _, block_id), block) = result?;
        num_returned += 1;
        let mut flags = Vec::new();
        if block.is_committed() {
            flags.push("✅");
        } else if block.has_justify_qc() {
            flags.push("🟡");
        } else {
            //
        }
        if block.is_dummy() {
            flags.push("⚪️");
        }
        if locked_block
            .as_ref()
            .is_some_and(|locked| *locked.block_id() == block_id)
        {
            flags.push("🔒️");
        }
        let flags = flags.join(" ");
        table.add_row(json!({
            "id": block.id(),
            "block_id": block.id(),
            "flags": flags,
            "height": block.height(),
            "epoch": block.epoch(),
            "shard_group": format!("{}-{}", block.shard_group().start().as_u32(), block.shard_group().end().as_u32()),
            "num_commands": block.commands().len(),
            "commands": block.commands(),
            "justify": block.justify().as_high_pc(),
            "timeout": block.timeout_certificate(),
            "proposed_by": block.proposed_by(),
            "parent": block.header().parent(),
            "commit_qc": block.commit_qc_id().map(hex::encode),
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

fn parse_query(query: &str) -> Result<Option<BlockQuery>, WebError> {
    if query.is_empty() {
        return Ok(None);
    }

    if let Some((key, value)) = query.split_once('=') {
        if key == "height" {
            return Ok(Some(BlockQuery::ByHeight(
                value
                    .parse::<u64>()
                    .map(NodeHeight)
                    .map_err(|e| WebError::bad_request(format!("Invalid height: {}", e)))?,
            )));
        }
    }

    let prefix = decode_hex_prefix(query)?;
    Ok(Some(BlockQuery::ByBlockIdPrefix(prefix)))
}

pub enum BlockQuery {
    ByHeight(NodeHeight),
    ByBlockIdPrefix(Vec<u8>),
}
