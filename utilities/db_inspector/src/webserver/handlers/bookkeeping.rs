//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{Extension, Json, extract::Path};
use serde_json::json;
use tari_ootle_common_types::optional::Optional;
use tari_state_store_rocksdb::{codecs::ByteColumn, column_families, read_only_ctx::ReadOnlyContext, traits::Cf};

use crate::webserver::{
    context::HandlerContext,
    error::WebError,
    handlers::types::{Column, TableResponse},
};

pub async fn list(
    Extension(context): Extension<Arc<HandlerContext>>,
    Path(db_name): Path<String>,
) -> Result<Json<TableResponse>, WebError> {
    const OPERATION: &str = "list_bookkeeping";
    let db = context.open_db(&db_name)?;
    let mut table = TableResponse::new([Column::new("name", "Name"), Column::new("contents", "Contents")]);
    let tx = db.read_only_context();

    fn add_item<CF, const N: u8>(
        tx: &ReadOnlyContext<'_>,
        name: &str,
        cf: CF,
        table_mut: &mut TableResponse,
    ) -> Result<(), WebError>
    where
        CF: Cf<Key = ByteColumn<N>>,
        CF::Value: serde::Serialize,
    {
        let cf = tx.cf(cf)?;
        let col = ByteColumn;
        let maybe_item = cf.get(&col, "list_bookkeeping").optional()?;
        table_mut.add_row(json!({
            "id": col.byte(),
            "name": name,
            "contents": maybe_item,
        }));
        Ok(())
    }

    add_item(&tx, "high_qc", column_families::bookkeeping::HighPcCf, &mut table)?;
    add_item(
        &tx,
        "last_sent_new_view",
        column_families::bookkeeping::LastSentNewViewCf,
        &mut table,
    )?;
    add_item(&tx, "high_tc", column_families::bookkeeping::HighTcCf, &mut table)?;
    add_item(
        &tx,
        "locked_block",
        column_families::bookkeeping::LockedBlockCf,
        &mut table,
    )?;
    add_item(&tx, "leaf_block", column_families::bookkeeping::LeafBlockCf, &mut table)?;
    add_item(
        &tx,
        "last_proposed_block",
        column_families::bookkeeping::LastProposedCf,
        &mut table,
    )?;
    add_item(
        &tx,
        "last_executed_block",
        column_families::bookkeeping::LastExecutedCf,
        &mut table,
    )?;
    add_item(
        &tx,
        "last_voted_block",
        column_families::bookkeeping::LastVotedCf,
        &mut table,
    )?;
    add_item(
        &tx,
        "last_sent_vote",
        column_families::bookkeeping::LastSentVoteCf,
        &mut table,
    )?;
    add_item(
        &tx,
        "highest_seen_block",
        column_families::bookkeeping::HighestSeenBlockCf,
        &mut table,
    )?;
    add_item(
        &tx,
        "commit_block",
        column_families::bookkeeping::CommitBlockCf,
        &mut table,
    )?;
    add_item(
        &tx,
        "last_sent_new_view",
        column_families::bookkeeping::LastSentNewViewCf,
        &mut table,
    )?;

    // Count the total number of entries in the bookkeeping column family (HighQcCf chosen arbitrarily)
    let cf = tx.cf(column_families::bookkeeping::HighPcCf)?;
    let total = cf.count(OPERATION)?;
    table.set_total_entries(total);

    Ok(Json(table))
}
