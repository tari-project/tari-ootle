//   Copyright 2024 The Tari Project
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

use crate::webserver::{context::HandlerContext, handlers};

const LOG_TARGET: &str = "tari::dan::swarm::webserver";

macro_rules! add_cf_route {
    ($api:expr, $cf:expr) => {
        $api = $api.route(
            &format!("/databases/:db_name/column-families/{}", $cf.as_name()),
            get(handlers::tables::list(|| $cf)),
        );
    };
}

pub async fn run(context: HandlerContext) -> anyhow::Result<()> {
    let bind_address = context.config().webserver.bind_address;

    let mut api = Router::new().route("/databases", get(handlers::databases::list)).route(
        "/databases/:db_name/column-families",
        get(handlers::databases::list_column_families),
    )
    // Special handling for blocks
    .route(
        "/databases/:db_name/column-families/blocks",
        get(handlers::blocks::list),
    );

    add_cf_route!(api, models::transaction::TransactionModel);
    add_cf_route!(api, models::transaction::FinalizedAtIndex);
    add_cf_route!(api, models::substate::HeadIndex);
    add_cf_route!(api, models::substate::SubstateModel);
    add_cf_route!(api, models::substate::UnprunedDownedValuesIndex);
    add_cf_route!(api, models::block::EpochHeightIndex);
    add_cf_route!(api, models::bookkeeping::HighQcModel); // TODO: Special case
    add_cf_route!(api, models::chain::PendingChainIndex);
    add_cf_route!(api, models::vote::VoteModel);
    add_cf_route!(api, models::foreign_proposal::ForeignProposalModel);
    add_cf_route!(api, models::foreign_proposal::EpochIndex);
    add_cf_route!(api, models::foreign_proposal::ProposedInBlockIndex);
    add_cf_route!(api, models::foreign_proposal::UnconfirmedIndex);
    add_cf_route!(api, models::quorum_certificate::QuorumCertificateModel);
    add_cf_route!(api, models::transaction_pool::TransactionPoolModel);
    // TODO: more

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
