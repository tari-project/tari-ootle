//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::net::SocketAddr;

use axum::{
    Extension,
    Router,
    middleware,
    routing::{get, post},
};
use log::*;
use tari_ootle_app_utilities::tcp::try_bind_with_fallback;
use tari_shutdown::ShutdownSignal;
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[cfg(feature = "metrics")]
use crate::rest_api::metrics;
use crate::{
    bootstrap::Services,
    rest_api::{
        context::HandlerContext,
        handlers,
        rate_limit::{self, RateLimitManager},
    },
};

const LOG_TARGET: &str = "tari::ootle::indexer::rest_api::server";

// Limit the body size to 4MB to allow for large transactions (wasm uploads)
const REQUEST_BODY_LIMIT: usize = 4 * 1024 * 1024; // 4 MB

#[derive(OpenApi)]
#[openapi(paths(
    handlers::misc::get_identity,
    handlers::misc::wait_until_ready,
    handlers::misc::get_epoch_manager_stats,
    handlers::network::get,
    handlers::network::get_network_sync_stats,
    handlers::network::get_connections,
    handlers::substates::get_substate,
    handlers::substates::fetch_substates,
    handlers::nfts::get_non_fungibles,
    handlers::resources::get_tari,
    handlers::resources::get_resource,
    handlers::indexer_events::sse_events,
    handlers::transactions::submit_transaction,
    handlers::transactions::submit_transaction_dry_run,
    handlers::transactions::list_recent_transactions,
    handlers::transactions::get_transaction_result,
    handlers::templates::get_template_definition,
    handlers::templates::list_cached_templates,
    handlers::templates::list_template_catalogue,
    handlers::templates::get_template_catalogue_entry,
    handlers::utxos::fetch_utxos,
    handlers::utxos::list_utxos,
    handlers::utxos::stream_utxo_updates,
    handlers::transaction_receipts::list_transaction_receipts,
    handlers::transaction_receipts::get_transaction_receipt,
    handlers::transaction_events::sse_transaction_events
))]
pub struct ApiDoc;

pub struct Server {
    #[cfg(feature = "metrics")]
    registry: prometheus_client::registry::Registry,
}

impl Server {
    #[cfg(not(feature = "metrics"))]
    pub fn new() -> Self {
        Self {}
    }

    #[cfg(feature = "metrics")]
    pub fn new(registry: prometheus_client::registry::Registry) -> Self {
        Self { registry }
    }

    pub async fn spawn(
        #[allow(unused_mut)] mut self,
        preferred_addr: SocketAddr,
        services: &Services,
        shutdown: ShutdownSignal,
    ) -> anyhow::Result<SocketAddr> {
        let rate_limit_manager = RateLimitManager::new(services.config.indexer.rate_limit.clone());
        let context = HandlerContext::from_services(services, rate_limit_manager);

        let router = Router::new()
            .route("/health", get(handlers::misc::health))
            .route("/ready", get(handlers::misc::ready))
            .route("/identity", get(handlers::misc::get_identity))
            .route("/wait-until-ready", get(handlers::misc::wait_until_ready))
            .route("/epoch-manager/stats", get(handlers::misc::get_epoch_manager_stats))
            .route("/network", get(handlers::network::get))
            .route("/network/stats", get(handlers::network::get_network_sync_stats))
            .route("/network/connections", get(handlers::network::get_connections))
            .nest("/substates", Router::new()
                .route("/fetch", post(handlers::substates::fetch_substates)
                    .layer(middleware::from_fn_with_state(context.clone(), rate_limit::substates_fetch_limit))
                )
                .route("/{substate_id}", get(handlers::substates::get_substate))
            )
            .nest("/transactions", Router::new()
                .route("/", post(handlers::transactions::submit_transaction)
                    .layer(middleware::from_fn_with_state(context.clone(), rate_limit::transactions_limit))
                )
                .route("/dry-run", post(handlers::transactions::submit_transaction_dry_run)
                    .layer(middleware::from_fn_with_state(context.clone(), rate_limit::dry_run_limit))
                )
                .route(
                    "/recent",
                    get(handlers::transactions::list_recent_transactions),
                )
                .route(
                    "/{transaction_id}/result",
                    get(handlers::transactions::get_transaction_result),
                )
                .route("/events", get(handlers::transactions::query_transaction_events))
                .route("/events/stream", get(handlers::transaction_events::sse_transaction_events)
                    .layer(middleware::from_fn_with_state(context.clone(), rate_limit::sse_limit))
                )
            )
            .nest("/templates", Router::new()
                .route("/cached", get(handlers::templates::list_cached_templates))
                .route("/catalogue", get(handlers::templates::list_template_catalogue))
                .route(
                    "/catalogue/{template_address}",
                    get(handlers::templates::get_template_catalogue_entry),
                )
                .route(
                    "/{template_address}",
                    get(handlers::templates::get_template_definition),
                )
            )
            .route("/non-fungibles", get(handlers::nfts::get_non_fungibles)
                .layer(middleware::from_fn_with_state(context.clone(), rate_limit::non_fungibles_limit))
            )
            .nest("/utxos", Router::new()
                .route("/", get(handlers::utxos::list_utxos))
                .route("/fetch", post(handlers::utxos::fetch_utxos)
                    .layer(middleware::from_fn_with_state(context.clone(), rate_limit::utxos_fetch_limit))
                )
                .route("/stream", post(handlers::utxos::stream_utxo_updates)
                    .layer(middleware::from_fn_with_state(context.clone(), rate_limit::sse_limit))
                )
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
            .nest("/resources/", Router::new()
                // Convenience Shortcut
                .route("/xtr" , get(handlers::resources::get_tari))
                .route("/tari" , get(handlers::resources::get_tari))
                .route("/{resource_address}" , get(handlers::resources::get_resource)))
            .route("/events", get(handlers::indexer_events::sse_events)
                .layer(middleware::from_fn_with_state(context.clone(), rate_limit::sse_limit))
            )
            .layer(CorsLayer::permissive())
            .layer(RequestBodyLimitLayer::new(REQUEST_BODY_LIMIT))
            .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", ApiDoc::openapi()))
            .layer(Extension(context.clone()))
            .layer(TraceLayer::new_for_http());

        #[cfg(feature = "metrics")]
        let router = router
            .layer(axum::middleware::from_fn_with_state(
                metrics::register(&mut self.registry),
                metrics::layer,
            ))
            .route("/_metrics", get(metrics::MetricsHandler::new(self.registry)));

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
