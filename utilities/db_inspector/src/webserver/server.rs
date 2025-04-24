//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{str::FromStr, sync::Arc};

use axum::{
    http::{HeaderValue, Response, Uri},
    response::IntoResponse,
    routing::get,
    Extension,
    Router,
};
use include_dir::{include_dir, Dir};
use log::*;
use reqwest::{header, StatusCode};
use tari_state_store_rocksdb::{models, traits::Cf};
use tower_http::cors::CorsLayer;

use crate::webserver::{context::HandlerContext, either_cf::EitherCf, handlers};

const LOG_TARGET: &str = "tari::dan::swarm::webserver";

macro_rules! add_cf_route {
    ($api:expr, $cf:expr) => {
        $api = $api.route(
            &format!("/databases/:db_name/column-families/{}", $cf.as_name()),
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
            "/databases/:db_name/column-families",
            get(handlers::databases::list_column_families),
        )
        // Special cases
        .route(
            "/databases/:db_name/column-families/blocks",
            get(handlers::blocks::list),
        )
        .route(
            "/databases/:db_name/column-families/state_transitions",
            get(handlers::state_transitions::list),
        )
        .route(
            "/databases/:db_name/column-families/block_diff",
            get(handlers::block_diff::list),
        )
        .route(
            "/databases/:db_name/column-families/bookkeeping",
            get(handlers::bookkeeping::list),
        );

    add_cf_routes!(
        api,
        models::vote::VoteModel,
        models::chain::PendingChainIndex,
        models::chain::CommittedParentChildChainIndex,
        models::chain::PendingParentChildIndex,
        models::foreign_proposal::ForeignProposalModel,
        models::foreign_proposal::ProposedInBlockIndex,
        models::foreign_proposal::EpochIndex,
        models::foreign_proposal::UnconfirmedIndex,
        models::block::EpochHeightIndex,
        // models::block_diff::BlockDiffModel,
        models::block_diff::SubstateIdIndex,
        models::quorum_certificate::QuorumCertificateModel,
        models::quorum_certificate::QuorumCertificateBlockIndex,
        models::block_transaction_execution::BlockTransactionExecutionModel,
        models::block_transaction_execution::TransactionIndex,
        models::transaction::TransactionModel,
        models::transaction_pool::TransactionPoolModel,
        models::transaction_pool_state_update::TransactionPoolStateUpdateModel,
        models::missing_transactions::MissingTransactionModel,
        models::parked_block::ParkedBlockModel,
        models::foreign_parked_blocks::ForeignParkedBlockModel,
        models::foreign_parked_blocks::MissingTransactionsModel,
        models::substate_locks::SubstateLockModel,
        models::substate_locks::HeadIndex,
        models::substate_locks::BlockIdIndex,
        models::substate_locks::SubstateIdIndex,
        models::substate::SubstateModel,
        models::substate::HeadIndex,
        models::substate::UnprunedDownedValuesIndex,
        // models::state_transition::StateTransitionModel,
        models::state_transition::ShardSeqIndex,
        models::foreign_substate_pledge::ForeignSubstatePledgeModel,
        models::pending_state_tree_diff::PendingStateTreeDiffModel,
        models::state_tree::StateTreeModel,
        models::state_tree::StateTreeStaleNodesModel,
        models::state_tree_shard_versions::StateTreeShardVersionModel,
        models::epoch_checkpoint::EpochCheckpointModel,
        models::burnt_utxo::BurntUtxoModel,
        models::burnt_utxo::ProposedInBlockIndex,
        EitherCf::<models::lock_conflict::LockConflictModel, models::lock_conflict::LockConflictBlockIdIndex>::new(),
        models::evicted_node::EvictedNodeModel,
        models::validator_node_epoch_stats::ValidatorNodeEpochStatsModel
    );

    let api = api.fallback(handlers::not_found);

    let router = Router::new()
        .nest("/api", api)
        .fallback(fallback_handler)
        .layer(CorsLayer::permissive())
        .layer(Extension(Arc::new(context)));

    let server = axum::Server::try_bind(&bind_address).or_else(|_| {
        error!(
            target: LOG_TARGET,
            "🕸️ Failed to bind on preferred address {}. Trying OS-assigned", bind_address
        );
        axum::Server::try_bind(&"127.0.0.1:0".parse().unwrap())
    })?;

    let server = server.serve(router.into_make_service());
    info!(target: LOG_TARGET, "🕸️ Webserver listening on http://{}", server.local_addr());
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
