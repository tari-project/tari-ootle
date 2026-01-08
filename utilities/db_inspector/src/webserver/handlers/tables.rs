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
use serde_json::{json, value::Index};
use tari_ootle_common_types::displayable::Displayable;
use tari_ootle_storage::{consensus_models::Evidence, Ordering};
use tari_state_store_rocksdb::{
    codecs::{DbCodec, KeyPrefix},
    error::RocksDbStorageError,
    traits::Cf,
};

use crate::webserver::{
    context::HandlerContext,
    error::WebError,
    handlers::types::{decode_hex_prefix, Column, TableRequest, TableResponse},
};

pub fn list<CF, F, S>(cf: F) -> impl Handler<(), S>
where
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
                let mut table = create_table_for_cf::<CF>();
                let transformer = create_transformer::<CF>();

                let tx = db.read_only_context();
                let cf = tx.cf(cf())?;
                let key_codec = CF::key_codec();
                let has_key_prefix = CF::key_prefix().is_some();
                let ordering = if req.desc {
                    Ordering::Descending
                } else {
                    Ordering::Ascending
                };
                #[allow(clippy::type_complexity)]
                let iter: Box<dyn Iterator<Item = Result<(CF::Key, CF::Value), RocksDbStorageError>>> =
                    if let Some(prefix_hex) = req.query.as_ref().filter(|q| !q.is_empty()) {
                        let key_prefix = decode_hex_prefix::<CF>(prefix_hex)?;
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

                    let value = serde_json::to_value(value).map_err(|e| anyhow!("Failed to serialize value: {}", e))?;
                    let mut value = transformer(value).map_err(|e| anyhow!("Failed to transform value: {}", e))?;
                    let key = key_codec
                        .encode(&key)
                        .map_err(|e| anyhow!("Failed to encode key: {}", e))?;
                    if !value.is_object() {
                        // Must return an object
                        value = json!({
                            "value": value,
                        });
                    }

                    value.set_value(
                        "id",
                        serde_json::Value::String(hex::encode(&key[usize::from(has_key_prefix)..])),
                    );
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
fn create_table_for_cf<CF: Cf>() -> TableResponse {
    let mut table = TableResponse::empty();

    let cf_name = CF::name();
    if cf_name == "bookkeeping" {
        table.with_columns([
            Column::new("block_id", "Block ID"),
            Column::new("epoch", "Epoch"),
            Column::new("height", "Height"),
        ]);
        return table;
    }

    let Some(prefix) = CF::key_prefix().map(|p| KeyPrefix::from_repr(p).expect("invalid KeyPrefix")) else {
        return table;
    };

    match prefix {
        KeyPrefix::Blocks => {
            table.with_columns([
                Column::new("block_id", "Block ID"),
                Column::new("epoch", "Epoch"),
                Column::new("height", "Height"),
                Column::new("commands", "Cmds"),
                Column::new("flags", "Flags"),
            ]);
        },
        KeyPrefix::Transactions => {
            table.with_columns([
                Column::new("transaction.V1.id", "Tx Id"),
                Column::new("transaction.V1.body.transaction.instructions", "Instructions"),
                Column::new("transaction.V1.body.transaction.fee_instructions", "Fee Instructions"),
                Column::new("transaction.V1.body.transaction.inputs", "Inputs"),
                Column::new("transaction.V1.body.signatures", "Signatures"),
                Column::new("transaction.V1.seal_signature.public_key", "Seal signer"),
            ]);
        },
        KeyPrefix::ForeignProposals => {
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
        KeyPrefix::ForeignProposalsEpochIndex => {
            table.with_columns([
                Column::new("block_id", "Block ID"),
                Column::new("proposed_in_block", "Proposed in"),
            ]);
        },
        KeyPrefix::ProposalCertificates => {
            table.with_columns([
                Column::new("height", "Height"),
                Column::new("header_hash", "Header hash"),
                Column::new("parent_id", "Parent ID"),
                Column::new("epoch", "Epoch"),
                Column::new("decision", "Decision"),
                Column::new("num_signatures", "# sigs"),
            ]);
        },
        KeyPrefix::TransactionPool => {
            table.with_columns([
                Column::new("transaction_id", "Transaction ID"),
                Column::new("summary", "Summary"),
                Column::new("evidence", "Evidence"),
                Column::new("transaction_fee", "Transaction Fee"),
                Column::new("leader_fee", "Leader Fee"),
                Column::new("stage", "Stage"),
                Column::new("pending_stage", "Pending Stage"),
                Column::new("original_decision", "Original Decision"),
                Column::new("local_decision", "Local Decision"),
                Column::new("remote_decision", "Remote Decision"),
                Column::new("locked_epoch", "Locked Epoch"),
                Column::new("is_global", "Is Global"),
                Column::new("is_ready", "Is Ready"),
            ]);
        },
        KeyPrefix::Substates => {
            table.with_columns([
                Column::new("substate_id", "Substate ID"),
                Column::new("version", "Version"),
                Column::new("substate_value", "Substate Value"),
                Column::new("state_hash", "State Hash"),
                Column::new("created", "Created"),
                Column::new("destroyed", "Destroyed"),
            ]);
        },
        _ => {
            // The data can still be returned to the request, though the columns will not be specified
        },
    }

    table
}

fn create_transformer<CF: Cf>() -> Box<dyn Fn(serde_json::Value) -> anyhow::Result<serde_json::Value>> {
    match CF::key_prefix() {
        Some(n) if n == KeyPrefix::TransactionPool.as_u8() => Box::new(|mut v| {
            let stage = v.get("pending_stage").unwrap().as_str().unwrap_or_else(|| {
                v.get("stage")
                    .unwrap()
                    .as_str()
                    .unwrap_or_else(|| panic!("{} Stage should be a string", CF::name()))
            });
            let evidence: Evidence = serde_json::from_value(v["evidence"].clone())?;
            let summary = evidence
                .iter()
                .map(|(shard_group, ev)| {
                    let num_outputs = ev.outputs().len();
                    let num_inputs = ev.inputs().len();
                    let prepared = ev.prepare_qc();
                    let accept = ev.accept_qc();
                    let ok_sym = match stage {
                        "New" => {
                            if num_inputs > 0 {
                                if prepared.is_some() {
                                    "✅ Prepared "
                                } else {
                                    "🔴"
                                }
                            } else {
                                "🌟"
                            }
                        },
                        "LocalPrepared" => {
                            if num_inputs > 0 {
                                if prepared.is_some() {
                                    "✅ Prepared "
                                } else {
                                    "🔴"
                                }
                            } else {
                                // Only expect LocalPrepared for local evidence, otherwise we're waiting for the foreign
                                // shard
                                "⌛️"
                            }
                        },
                        "LocalAccepted" => {
                            if accept.is_some() {
                                "🟢 Accepted "
                            } else {
                                "🔴"
                            }
                        },
                        s => s,
                    };

                    format!(
                        "[[ {}{}: in: {} | out: {} | p: {} | a: {} ]] ",
                        ok_sym,
                        shard_group.to_parsable_string(),
                        num_inputs,
                        num_outputs,
                        prepared.display(),
                        accept.display(),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            v["summary"] = serde_json::Value::String(summary);
            Ok(v)
        }),

        Some(n) if n == KeyPrefix::ProposalCertificates.as_u8() => Box::new(|mut v| {
            v["num_signatures"] =
                serde_json::Value::Number(v["signatures"].as_array().map_or(0, |sigs| sigs.len()).into());
            Ok(v)
        }),
        _ => Box::new(Ok),
    }
}

/// If serde_json doesnt provide a way to set a value without using IndexMut, we can implement our own. This is to keep
/// clippy happy (indexing_slicing)
trait SetJsonValue {
    fn set_value(&mut self, key: &str, value: serde_json::Value);
}

impl SetJsonValue for serde_json::Value {
    fn set_value(&mut self, key: &str, value: serde_json::Value) {
        *key.index_or_insert(self) = value;
    }
}
