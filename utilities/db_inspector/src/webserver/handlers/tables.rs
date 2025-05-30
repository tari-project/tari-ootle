//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    extract::{Path, Query},
    handler::Handler,
    Extension,
    Json,
};
use serde::Serialize;
use serde_json::json;
use tari_dan_storage::Ordering;
use tari_state_store_rocksdb::{codecs::DbCodec, column_families, error::RocksDbStorageError, traits::Cf};

use crate::webserver::{
    context::HandlerContext,
    error::WebError,
    handlers::types::{decode_hex_prefix, Column, TableRequest, TableResponse},
};

pub fn list<CF, F, B, S>(cf: F) -> impl Handler<(), S, B>
where
    B: axum::body::HttpBody + Send + 'static,
    S: Clone + Send + Sync + 'static,
    F: Fn() -> CF + Send + Sync + Clone + 'static,
    CF: Cf + Send + Sync + 'static,
    CF::Value: Serialize,
{
    axum::routing::get(
        move |Extension(context): Extension<Arc<HandlerContext>>,
              Path(db_name): Path<String>,
              Query(req): Query<TableRequest>| {
            async move {
                const OPERATION: &str = "list_cf";
                let db = context.open_db(&db_name)?;
                let mut table = create_table_for_cf(CF::name());

                let tx = db.read_only_context();
                let cf = tx.cf(cf())?;
                let key_codec = CF::key_codec();
                let ordering = if req.asc {
                    Ordering::Ascending
                } else {
                    Ordering::Descending
                };
                #[allow(clippy::type_complexity)]
                let iter: Box<dyn Iterator<Item = Result<(CF::Key, CF::Value), RocksDbStorageError>>> =
                    if let Some(prefix_hex) = req.query_prefix_hex.as_ref() {
                        let key_prefix = decode_hex_prefix(prefix_hex)?;
                        Box::new(cf.prefix_range_iterator_raw_key(ordering, key_prefix))
                    } else {
                        Box::new(cf.iterator(ordering, OPERATION))
                    };

                let page_size = req.limit.unwrap_or(1_000);
                let skip = req.page.unwrap_or(0) * page_size;

                let mut total_bytes = 0usize;
                let mut largest_row_size = 0usize;
                let mut smallest_row_size = usize::MAX;
                for result in iter.skip(skip).take(page_size) {
                    let (key, value) = result?;
                    let bytes = cf
                        .get_raw_pinned(&key, OPERATION)
                        .map_err(|e| anyhow!("Failed to get_raw_pinned: {}", e))?;
                    let size = bytes.map(|b| b.len()).unwrap_or(0);
                    total_bytes += size;
                    if size > largest_row_size {
                        largest_row_size = size;
                    }
                    if size < smallest_row_size {
                        smallest_row_size = size;
                    }

                    let mut value =
                        serde_json::to_value(value).map_err(|e| anyhow!("Failed to serialize value: {}", e))?;
                    let key = key_codec
                        .encode(&key)
                        .map_err(|e| anyhow!("Failed to encode key: {}", e))?;
                    if !value.is_object() {
                        // Must return an object
                        value = json!({
                            "value": value,
                        });
                    }
                    value["id"] = serde_json::Value::String(hex::encode(key));
                    table.add_row(value);
                }
                let total = cf.count(OPERATION)?;
                table
                    .set_total_entries(total)
                    .set_total_bytes(total_bytes)
                    .set_largest_row_size(largest_row_size)
                    .set_smallest_row_size(smallest_row_size);

                Ok::<_, WebError>(Json(table))
            }
        },
    )
}

#[allow(clippy::too_many_lines)]
fn create_table_for_cf(cf_name: &str) -> TableResponse {
    let mut table = TableResponse::empty();

    match cf_name {
        "bookkeeping" => {
            table.with_columns([
                Column::new("block_id", "Block ID"),
                Column::new("epoch", "Epoch"),
                Column::new("height", "Height"),
            ]);
        },
        "blocks" => {
            table.with_columns([
                Column::new("block_id", "Block ID"),
                Column::new("epoch", "Epoch"),
                Column::new("height", "Height"),
                Column::new("commands", "Cmds"),
                Column::new("flags", "Flags"),
            ]);
        },
        "transactions" => {
            table.with_columns([
                Column::new("transaction.V1.id", "Tx Id"),
                Column::new("transaction.V1.body.transaction.instructions", "Instructions"),
                Column::new("transaction.V1.body.transaction.fee_instructions", "Fee Instructions"),
                Column::new("transaction.V1.body.transaction.inputs", "Inputs"),
                Column::new("transaction.V1.body.signatures", "Signatures"),
                Column::new("transaction.V1.seal_signature.public_key", "Seal signer"),
            ]);
        },
        "votes" => {
            table.with_columns([
                Column::new("epoch", "Epoch"),
                Column::new("block_id", "Block ID"),
                Column::new("decision", "Decision"),
                Column::new("sender_leaf_hash", "Sender Leaf Hash"),
                Column::new("signature", "Signature"),
            ]);
        },
        "foreign_proposals" => {
            table.with_columns([
                Column::new("proposal.commit_proof.V1.commit_proof.header.epoch", "Epoch"),
                Column::new("proposal.commit_proof.V1.commit_proof.header.height", "Height"),
                Column::new(
                    "proposal.commit_proof.V1.commit_proof.header.shard_group",
                    "Shard group",
                ),
                Column::new("proposal.commit_proof.V1.commands", "Commands"),
                Column::new("proposal.commit_proof.V1.commit_proof", "Commit Proof"),
                Column::new("proposal.block_pledge.pledges", "Pledges"),
                Column::new("proposed_in_block", "Proposed In"),
                Column::new("status", "Status"),
            ]);
        },
        "foreignproposals_epoch_idx" => {
            table.with_columns([
                Column::new("block_id", "Block ID"),
                Column::new("proposed_in_block", "Proposed in"),
            ]);
        },
        s if s == column_families::certificates::proposal::ProposalCertificateCf::name() => {
            table.with_columns([
                Column::new("qc_id", "QC ID"),
                Column::new("block_id", "Block ID"),
                Column::new("header_hash", "Header hash"),
                Column::new("parent_id", "Parent ID"),
                Column::new("epoch", "Epoch"),
                Column::new("decision", "Decision"),
            ]);
        },
        "transaction_pool" => {
            table.with_columns([
                Column::new("transaction_id", "Transaction ID"),
                Column::new("evidence", "Evidence"),
                Column::new("transaction_fee", "Transaction Fee"),
                Column::new("leader_fee", "Leader Fee"),
                Column::new("stage", "Stage"),
                Column::new("pending_stage", "Pending Stage"),
                Column::new("original_decision", "Original Decision"),
                Column::new("local_decision", "Local Decision"),
                Column::new("remote_decision", "Remote Decision"),
                Column::new("is_global", "Is Global"),
                Column::new("is_ready", "Is Ready"),
            ]);
        },
        "substates" => {
            table.with_columns([
                Column::new("substate_id", "Substate ID"),
                Column::new("version", "Version"),
                Column::new("substate_value", "Substate Value"),
                Column::new("state_hash", "State Hash"),
                Column::new("created_by_shard", "Created By Shard"),
                Column::new("created_at_epoch", "Created At Epoch"),
                Column::new("destroyed", "Destroyed"),
            ]);
        },
        _ => {
            // The data can still be returned to the request, though the columns will not be specified
        },
    }

    table
}
