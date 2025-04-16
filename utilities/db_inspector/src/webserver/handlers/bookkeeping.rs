//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{extract::Path, Extension, Json};
use serde_json::json;
use tari_dan_common_types::optional::Optional;
use tari_state_store_rocksdb::{codecs::ByteColumn, models, read_only::ReadOnlyContext, traits::Cf};

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
        if let Some(item) = cf.get(&col, "list_bookkeeping").optional()? {
            table_mut.add_row(json!({
                "id": col.byte(),
                "name": name,
                "contents": item,
            }));
        }
        Ok(())
    }

    add_item(&tx, "high_qc", models::bookkeeping::HighQcModel, &mut table)?;
    add_item(&tx, "locked_block", models::bookkeeping::LockedBlockModel, &mut table)?;
    add_item(&tx, "leaf_block", models::bookkeeping::LeafBlockModel, &mut table)?;
    add_item(
        &tx,
        "last_proposed_block",
        models::bookkeeping::LastProposedModel,
        &mut table,
    )?;
    add_item(
        &tx,
        "last_executed_block",
        models::bookkeeping::LastExecutedModel,
        &mut table,
    )?;
    add_item(&tx, "last_voted_block", models::bookkeeping::LastVotedModel, &mut table)?;
    add_item(
        &tx,
        "last_sent_vote",
        models::bookkeeping::LastSentVoteModel,
        &mut table,
    )?;
    add_item(&tx, "commit_block", models::bookkeeping::CommitBlockModel, &mut table)?;

    // Count the total number of entries in the bookkeeping column family (HighQcModel chosen arbitrarily)
    let cf = tx.cf(models::bookkeeping::HighQcModel)?;
    let total = cf.count(OPERATION)?;
    table.set_total_entries(total);

    Ok(Json(table))
}
