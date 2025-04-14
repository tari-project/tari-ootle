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
use tari_dan_storage::Ordering;
use tari_state_store_rocksdb::{codecs::DbCodec, models, traits::Cf};

use crate::webserver::{
    context::HandlerContext,
    error::WebError,
    handlers::types::{Column, TableRequest, TableResponse},
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
                let mut table = create_table_for_cf(CF::name())?;

                let tx = db.read_only_context();
                let cf = tx.cf(cf())?;
                let key_codec = CF::key_codec();
                let ordering = if req.asc {
                    Ordering::Ascending
                } else {
                    Ordering::Descending
                };
                let iter = cf.iterator(ordering, OPERATION);
                for result in iter.take(req.limit.unwrap_or(usize::MAX)) {
                    let (key, value) = result?;
                    let mut value =
                        serde_json::to_value(value).map_err(|e| anyhow!("Failed to serialize value: {}", e))?;
                    let key = key_codec
                        .encode(&key)
                        .map_err(|e| anyhow!("Failed to encode key: {}", e))?;
                    value["id"] = serde_json::Value::String(hex::encode(key));
                    table.add_row(value);
                }
                let total = cf.count(OPERATION)?;
                table.set_total_entries(total);

                Ok::<_, WebError>(Json(table))
            }
        },
    )
}

fn create_table_for_cf(cf_name: &str) -> Result<TableResponse, WebError> {
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
                Column::new("block.header.id", "Block ID"),
                Column::new("block.header.epoch", "Epoch"),
                Column::new("block.height", "Height"),
                Column::new("block.commands", "Cmds"),
                Column::new("proposed_in_block", "Proposed In"),
                Column::new("status", "Status"),
            ]);
        },
        "foreignproposals_proposed_idx" => {
            // No columns to show - we're interested in the key (automatically added)
        },
        "foreignproposals_epoch_idx" => {
            table.with_columns([
                Column::new("block_id", "Block ID"),
                Column::new("proposed_in_block", "Proposed in"),
            ]);
        },
        "foreignproposals_unconfirmed_idx" => {
            // No columns to show - we're interested in the key
        },
        "block_height_idx" => {
            // No columns to show - we're interested in the key
        },
        s if s == models::quorum_certificate::QuorumCertificateModel::name() => {
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
            return Err(WebError::bad_request(format!(
                "Unknown/unsupported column family {cf_name}"
            )))
        },
    }

    Ok(table)
}

//         block::EpochHeightIndex::name(),
//         BlockDiffModel::name(),
//         block_diff::SubstateIdIndex::name(),
//         QuorumCertificateModel::name(),
//         BlockTransactionExecutionModel::name(),
//         block_transaction_execution::TransactionIndex::name(),
//         TransactionModel::name(),
//         transaction::FinalizedAtIndex::name(),
//         TransactionPoolModel::name(),
//         TransactionPoolStateUpdateModel::name(),
//         MissingTransactionModel::name(),
//         ParkedBlockModel::name(),
//         ForeignParkedBlockModel::name(),
//         foreign_parked_blocks::MissingTransactionsModel::name(),
//         ForeignSendCounterModel::name(),
//         ForeignReceiveCounterModel::name(),
//         SubstateLockModel::name(),
//         substate_locks::HeadIndex::name(),
//         substate_locks::BlockIdIndex::name(),
//         substate_locks::SubstateIdIndex::name(),
//         SubstateModel::name(),
//         substate::HeadIndex::name(),
//         substate::UnprunedDownedValuesIndex::name(),
//         StateTransitionModel::name(),
//         state_transition::ShardSeqIndex::name(),
//         ForeignSubstatePledgeModel::name(),
//         PendingStateTreeDiffModel::name(),
//         StateTreeModel::name(),
//         StateTreeStaleNodesModelRef::name(),
//         StateTreeShardVersionModel::name(),
//         EpochCheckpointModel::name(),
//         BurntUtxoModel::name(),
//         burnt_utxo::ProposedInBlockIndex::name(),
//         LockConflictModel::name(),
//         lock_conflict::ByBlockIdQuery::name(),
//         EvictedNodeModel::name(),
//         ValidatorNodeEpochStatsModel::name(),
