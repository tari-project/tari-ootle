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
use tari_state_store_rocksdb::{column_families::state_tree::StateTreeCf, error::RocksDbStorageError, traits::Cf};

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
    const OPERATION: &str = "list_state_tree";
    let db = context.open_db(&db_name)?;
    let mut table = TableResponse::new([
        Column::new("shard", "Shard"),
        Column::new("key", "Key"),
        Column::new("type", "Type"),
        Column::new("leaf_count", "Leaf Count"),
        Column::new("substate_addr", "Substate Address"),
        Column::new("version", "Version"),
    ]);
    let tx = db.read_only_context();

    let cf = tx.cf(StateTreeCf)?;

    let ordering = if req.desc {
        Ordering::Descending
    } else {
        Ordering::Ascending
    };
    type Key = <StateTreeCf as Cf>::Key;
    type Value = <StateTreeCf as Cf>::Value;
    let iter: Box<dyn Iterator<Item = Result<(Key, Value), RocksDbStorageError>>> =
        if let Some(prefix_hex) = req.query.as_ref() {
            let key_prefix = decode_hex_prefix::<StateTreeCf>(prefix_hex)?;
            Box::new(cf.prefix_range_iterator_raw_key(ordering, key_prefix))
        } else {
            Box::new(cf.iterator(ordering, OPERATION))
        };

    let row_limit = req.limit.unwrap_or(1_000);
    let row_skip = req.page.unwrap_or(0).saturating_mul(row_limit);
    for result in iter.skip(row_skip).take(row_limit) {
        let ((shard, node_key), node) = result?;
        let encoded_key = cf.encode_key(&(shard, node_key.clone()));
        let key_hex = hex::encode(&encoded_key);
        match node {
            tari_jellyfish::Node::Internal(node) => {
                table.add_row(json!({
                    "id": key_hex,
                    "shard": shard,
                    "key": node_key.to_string(),
                    "type": "Internal",
                    "leaf_count": node.leaf_count(),
                    "substate_addr": "",
                    "version": node_key.version(),
                }));
            },
            tari_jellyfish::Node::Leaf(node) => {
                table.add_row(json!({
                    "id": key_hex,
                    "shard": shard,
                    "key": node_key.to_string(),
                    "type": "Leaf",
                    "leaf_count": 0,
                    "substate_addr": node.payload(),
                    "version": node.version(),
                }));
            },
            tari_jellyfish::Node::Null => {
                table.add_row(json!({
                    "id": key_hex,
                    "shard": shard,
                    "key": node_key.to_string(),
                    "type": "Null",
                    "leaf_count": 0,
                    "substate_addr": "",
                    "version": node_key.version(),
                }));
            },
        }
    }
    let count = cf.count(OPERATION)?;
    table.set_total_entries(count);

    Ok(Json(table))
}
