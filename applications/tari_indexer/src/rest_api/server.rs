//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{net::SocketAddr, time::Duration};

use axum::{
    Extension,
    Router,
    middleware,
    routing::{get, post},
};
use log::*;
use tari_ootle_app_utilities::tcp::try_bind_with_fallback;
use tari_shutdown::ShutdownSignal;
use tower::{ServiceBuilder, buffer::BufferLayer};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[cfg(feature = "metrics")]
use crate::rest_api::metrics;
use crate::{
    bootstrap::Services,
    rest_api::{context::HandlerContext, handlers, rate_limit},
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
    handlers::transaction_events::sse_transaction_events,
    handlers::epoch_checkpoints::list_epoch_checkpoints,
    handlers::epoch_checkpoints::get_latest_epoch_checkpoint
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
        let context = HandlerContext::from_services(services);
        let config = &services.config.indexer.rate_limit;

        let submit_transaction_limiter = rate_limit::RateLimiter::new(
            config.submit_transaction_per_minute,
            Duration::from_secs(60),
        );
        let fetch_substates_limiter = rate_limit::RateLimiter::new(
            config.fetch_substates_per_minute,
            Duration::from_secs(60),
        );
        let fetch_utxos_limiter = rate_limit::RateLimiter::new(
            config.fetch_utxos_per_minute,
            Duration::from_secs(60),
        );
        let get_nfts_limiter = rate_limit::RateLimiter::new(
            config.get_non_fungibles_per_minute,
            Duration::from_secs(60),
        );
        let dry_run_limiter = rate_limit::RateLimiter::new(
            config.submit_transaction_dry_run_per_5_seconds,
            Duration::from_secs(5),
        );
        let sse_limiter = rate_limit::ConcurrentConnectionLimiter::new(
            config.sse_max_concurrent_connections_per_ip,
        );

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
                    .layer(middleware::from_fn({
                        let limiter = fetch_substates_limiter.clone();
                        move |req, next| {
                            let limiter = limiter.clone();
                            async move { limiter.middleware(req, next).await }
                        }
                    })))
                .route("/watched", get(handlers::watched::list_watched_substates))
                .route("/{substate_id}", get(handlers::substates::get_substate))
            )
            .nest("/transactions", Router::new()
                .route("/", post(handlers::transactions::submit_transaction)
                    .layer(middleware::from_fn({
                        let limiter = submit_transaction_limiter.clone();
                        move |req, next| {
                            let limiter = limiter.clone();
                            async move { limiter.middleware(req, next).await }
                        }
                    })))
                .route("/dry-run", post(handlers::transactions::submit_transaction_dry_run)
                    .layer(middleware::from_fn({
                        let limiter = dry_run_limiter.clone();
                        move |req, next| {
                            let limiter = limiter.clone();
                            async move { limiter.middleware(req, next).await }
                        }
                    })))
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
                    .layer(middleware::from_fn({
                        let limiter = sse_limiter.clone();
                        move |req, next| {
                            let limiter = limiter.clone();
                            async move { limiter.middleware(req, next).await }
                        }
                    })))
            )
            .nest("/templates", Router::new()
                .route("/cached", get(handlers::templates::list_cached_templates))
                .route("/watched", get(handlers::watched::list_watched_templates))
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
                .layer(middleware::from_fn({
                    let limiter = get_nfts_limiter.clone();
                    move |req, next| {
                        let limiter = limiter.clone();
                        async move { limiter.middleware(req, next).await }
                    }
                })))
            .nest("/utxos", Router::new()
                .route("/", get(handlers::utxos::list_utxos))
                .route("/fetch", post(handlers::utxos::fetch_utxos)
                    .layer(middleware::from_fn({
                        let limiter = fetch_utxos_limiter.clone();
                        move |req, next| {
                            let limiter = limiter.clone();
                            async move { limiter.middleware(req, next).await }
                        }
                    })))
                .route("/stream", post(handlers::utxos::stream_utxo_updates)
                    .layer(middleware::from_fn({
                        let limiter = sse_limiter.clone();
                        move |req, next| {
                            let limiter = limiter.clone();
                            async move { limiter.middleware(req, next).await }
                        }
                    })))
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
            .nest("/epoch-checkpoints", Router::new()
                .route("/", get(handlers::epoch_checkpoints::list_epoch_checkpoints))
                .route("/latest", get(handlers::epoch_checkpoints::get_latest_epoch_checkpoint))
            )
            .route("/events", get(handlers::indexer_events::sse_events)
                .layer(middleware::from_fn({
                    let limiter = sse_limiter.clone();
                    move |req, next| {
                        let limiter = limiter.clone();
                        async move { limiter.middleware(req, next).await }
                    }
                })))
            .layer(CorsLayer::permissive())
            .layer(RequestBodyLimitLayer::new(REQUEST_BODY_LIMIT))
            .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", ApiDoc::openapi()))
            .layer(Extension(context))
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
