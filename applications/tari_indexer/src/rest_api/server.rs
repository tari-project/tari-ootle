//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::net::SocketAddr;

use axum::{
    routing::{get, post},
    Extension,
    Router,
};
use log::*;
use tari_ootle_app_utilities::tcp::try_bind_with_fallback;
use tari_shutdown::ShutdownSignal;
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    bootstrap::Services,
    rest_api::{context::HandlerContext, handlers},
};

const LOG_TARGET: &str = "tari::ootle::indexer::rest_api::server";

// Limit the body size to 4MB to allow for large transactions (wasm uploads)
const REQUEST_BODY_LIMIT: usize = 4 * 1024 * 1024; // 4 MB

#[derive(OpenApi)]
#[openapi(paths(
    handlers::misc::get_identity,
    handlers::misc::wait_until_ready,
    handlers::misc::get_epoch_manager_stats,
    handlers::network::get_network_sync_stats,
    handlers::network::get_connections,
    handlers::network::add_connection,
    handlers::substates::get_substate,
    handlers::substates::list_substates,
    handlers::substates::fetch_substates,
    handlers::transactions::submit_transaction,
    handlers::transactions::list_recent_transactions,
    handlers::transactions::get_transaction_result,
    handlers::templates::get_template_definition,
    handlers::templates::list_templates,
    handlers::utxos::fetch_utxos,
    handlers::utxos::list_utxos,
    handlers::utxos::stream_utxo_updates,
    handlers::transaction_receipts::list_transaction_receipts,
    handlers::transaction_receipts::get_transaction_receipt
))]
pub struct ApiDoc;

pub struct Server;

impl Server {
    pub async fn spawn(
        preferred_addr: SocketAddr,
        services: &Services,
        shutdown: ShutdownSignal,
    ) -> anyhow::Result<SocketAddr> {
        let context = HandlerContext::from_services(services);

        let router = Router::new()
            .route("/identity", get(handlers::misc::get_identity))
            .route("/wait-until-ready", get(handlers::misc::wait_until_ready))
            .route("/epoch-manager/stats", get(handlers::misc::get_epoch_manager_stats))
            .route("/network/stats", get(handlers::network::get_network_sync_stats))
            .route(
                "/network/connections",
                get(handlers::network::get_connections).post(handlers::network::add_connection),
            )
            .nest("/substates", Router::new()
                .route("/fetch", post(handlers::substates::fetch_substates))
                .route("/{substate_id}", get(handlers::substates::get_substate))
                .route("/", get(handlers::substates::list_substates))
            )
            .nest("/transactions", Router::new()
                .route("/", post(handlers::transactions::submit_transaction))
                .route(
                    "/recent",
                    get(handlers::transactions::list_recent_transactions),
                )
                .route(
                    "/{transaction_id}/result",
                    get(handlers::transactions::get_transaction_result),
                )
            )
            .nest("/templates", Router::new()
                .route(
                    "/{template_address}",
                    get(handlers::templates::get_template_definition),
                )
                .route("/", get(handlers::templates::list_templates))
            )
            .route("/non-fungibles", get(handlers::nfts::get_non_fungibles)) // Placeholder for future non-fungible endpoints
            .nest("/utxos", Router::new()
                .route("/", get(handlers::utxos::list_utxos))
                .route("/fetch", post(handlers::utxos::fetch_utxos))
                .route("/stream", post(handlers::utxos::stream_utxo_updates))
            )
            .nest(
                "/transaction-receipts",
                Router::new()
                    .route("/", get(handlers::transaction_receipts::list_transaction_receipts))
                    .route(
                        "/{address}",
                        get(handlers::transaction_receipts::get_transaction_receipt),
                    ),
            )
            .layer(CorsLayer::permissive())
            .layer(RequestBodyLimitLayer::new(REQUEST_BODY_LIMIT))
            .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", ApiDoc::openapi()))
            .layer(Extension(context));

        let listener = try_bind_with_fallback(preferred_addr).await?;

        // spawn server
        let listen_addr = listener.local_addr()?;
        info!(target: LOG_TARGET, "🌐 Indexer REST API server listening on {listen_addr}");
        tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router).with_graceful_shutdown(shutdown).await {
                error!(target: LOG_TARGET, "Wallet query HTTP server error: {error}");
            }
        });

        Ok(listen_addr)
    }
}
