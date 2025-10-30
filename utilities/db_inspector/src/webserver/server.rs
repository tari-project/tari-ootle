//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{str::FromStr, sync::Arc};

use axum::{
    http::{header, HeaderValue, Response, StatusCode, Uri},
    response::IntoResponse,
    routing::get,
    Extension,
    Router,
};
use include_dir::{include_dir, Dir};
use log::*;
use tari_state_store_rocksdb::{column_families, traits::Cf};
use tower_http::cors::CorsLayer;

use crate::webserver::{context::HandlerContext, handlers};

const LOG_TARGET: &str = "tari::ootle::swarm::webserver";

macro_rules! add_cf_route {
    ($api:expr, $cf:expr) => {
        $api = $api.route(
            &format!("/databases/{{db_name}}/column-families/{}", $cf.as_name()),
            get(handlers::tables::list(|| $cf)),
        );
    };
}
macro_rules! add_cf_routes {
    ($api:expr, $($cf:expr),+) => {
        $(add_cf_route!($api, $cf);)+
    };
}

pub async fn run(context: HandlerContext) -> anyhow::Result<()> {
    let bind_address = context.config().webserver.bind_address;

    let mut api = Router::new()
        .route("/databases", get(handlers::databases::list))
        .route(
            "/databases/{db_name}/column-families",
            get(handlers::column_families::list),
        )
        // Special cases
        .route(
            "/databases/{db_name}/column-families/blocks",
            get(handlers::blocks::list),
        )
        .route(
            "/databases/{db_name}/column-families/state_transitions",
            get(handlers::state_transitions::list),
        )
        .route(
            "/databases/{db_name}/column-families/block_diff",
            get(handlers::block_diff::list),
        )
        .route(
            "/databases/{db_name}/column-families/block_diff_substate_id_idx",
            get(handlers::block_diff_substate_id_index::list),
        )
        .route(
            "/databases/{db_name}/column-families/bookkeeping",
            get(handlers::bookkeeping::list),
        )
        .route(
            "/databases/{db_name}/column-families/foreign_substate_pledges",
            get(handlers::foreign_substate_pledges::list),
        );

    add_cf_routes!(
        api,
        column_families::chain::PendingChainIndex,
        column_families::chain::CommittedParentChildChainIndex,
        column_families::chain::PendingParentChildIndex,
        column_families::foreign_proposal::ForeignProposalCf,
        column_families::foreign_proposal::ProposedInBlockIndex,
        column_families::foreign_proposal::EpochIndex,
        column_families::foreign_proposal::UnconfirmedIndex,
        column_families::block::EpochHeightIndex,
        // column_families::block_diff::BlockDiffModel,
        // column_families::block_diff::SubstateIdIndex,
        column_families::certificates::proposal::ProposalCertificateCf,
        column_families::certificates::timeout::TimeoutCertificateCf,
        column_families::block_transaction_execution::BlockTransactionExecutionCf,
        column_families::block_transaction_execution::BlockIndex,
        column_families::finalized_transaction::FinalizedTransactionLinkCf,
        column_families::transaction::TransactionCf,
        column_families::transaction_pool::TransactionPoolCf,
        column_families::transaction_pool_state_update::TransactionPoolStateUpdateCf,
        column_families::transaction_pool_state_update::TransactionPoolStateUpdateDebugHistoryCf,
        column_families::missing_transactions::MissingTransactionCf,
        column_families::parked_block::ParkedBlockCf,
        column_families::foreign_parked_blocks::ForeignParkedBlockCf,
        column_families::foreign_parked_blocks::MissingTransactionsModel,
        column_families::substate_locks::SubstateLockModel,
        column_families::substate_locks::HeadIndex,
        column_families::substate_locks::BlockIdIndex,
        column_families::substate_locks::SubstateIdIndex,
        column_families::substate::SubstateCf,
        column_families::substate::HeadIndex,
        column_families::substate::UnprunedDownedValuesIndex,
        // column_families::state_transition::StateTransitionModel,
        // column_families::foreign_substate_pledge::ForeignSubstatePledgeModel,
        column_families::pending_state_tree_diff::PendingStateTreeDiffCf,
        column_families::state_tree::StateTreeCf,
        column_families::state_tree::StateTreeStaleNodesModel,
        column_families::state_tree_shard_versions::StateTreeShardVersionCf,
        column_families::epoch_checkpoint::EpochCheckpointCf,
        column_families::lock_conflict::LockConflictCf,
        column_families::lock_conflict::LockConflictBlockIdIndex,
        column_families::evicted_node::EvictedNodeCf,
        column_families::validator_node_epoch_stats::ValidatorNodeEpochStatsCf
    );

    let api = api.fallback(handlers::not_found);

    let router = Router::new()
        .nest("/api", api)
        .fallback(fallback_handler)
        .layer(CorsLayer::permissive())
        .layer(Extension(Arc::new(context)));

    let listener = match tokio::net::TcpListener::bind(bind_address).await {
        Ok(l) => l,
        Err(e) => {
            warn!(
                target: LOG_TARGET,
                "🕸️ Failed to bind on preferred address {} ({e}). Trying OS-assigned", bind_address
            );
            tokio::net::TcpListener::bind(("127.0.0.1", 0u16)).await?
        },
    };
    let server = axum::serve(listener, router);
    info!(target: LOG_TARGET, "🕸️ Webserver listening on http://{}", server.local_addr()?);
    server.await?;

    Ok(())
}

static WEB_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web_ui/dist");

async fn fallback_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path();

    // If path starts with /, strip it.
    let path = path.strip_prefix('/').unwrap_or(path);

    // If the path is a file, return it. Otherwise, use index.html (SPA)
    if let Some(body) = WEB_DIR
        .get_file(path)
        .or_else(|| WEB_DIR.get_file("index.html"))
        .and_then(|file| file.contents_utf8())
    {
        let mime_type = mime_guess::from_path(path).first_or_else(|| mime_guess::Mime::from_str("text/html").unwrap());
        return Response::builder()
            .header(header::CONTENT_TYPE, HeaderValue::from_str(mime_type.as_ref()).unwrap())
            .status(StatusCode::OK)
            .body(body.to_owned())
            .unwrap();
    }
    log::warn!(target: LOG_TARGET, "Not found {:?}", path);
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(String::new())
        .unwrap()
}
