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
use tower::{ServiceBuilder, buffer::BufferLayer, limit::RateLimitLayer};
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
        rate_limit::{IpRateLimiter, RateLimitConfig, SseConnectionLimiter, rate_limit_middleware, sse_limit_middleware},
    },
};

const LOG_TARGET: &str = "tari::ootle::indexer::rest_api::server";

// Limit the body size to 4MB to allow for large transactions (wasm uploads)
const REQUEST_BODY_LIMIT: usize = 4 * 1024 * 1024; // 4 MB

// ---------------------------------------------------------------------------
// Rate-limit configuration
// ---------------------------------------------------------------------------

/// POST /transactions – 20 requests per minute per IP.
const TX_SUBMIT_RATE_CAPACITY: u32 = 20;
const TX_SUBMIT_RATE_WINDOW_SECS: u64 = 60;

/// POST /substates/fetch and POST /utxos/fetch – 60 requests per minute per IP.
const FETCH_RATE_CAPACITY: u32 = 60;
const FETCH_RATE_WINDOW_SECS: u64 = 60;

/// GET /non-fungibles and GET /transactions/recent – 60 requests per minute per IP.
const READ_RATE_CAPACITY: u32 = 60;
const READ_RATE_WINDOW_SECS: u64 = 60;

/// Maximum concurrent SSE connections (across all IPs).
const SSE_MAX_CONNECTIONS: usize = 100;

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
        let context = HandlerContext::from_services(services);

        // Per-IP rate limiters for specific endpoint groups.
        // `trust_proxy_headers` is false by default – enable only when the
        // indexer is behind a trusted reverse proxy.
        let tx_submit_limiter = RateLimitConfig {
            limiter: IpRateLimiter::new(TX_SUBMIT_RATE_CAPACITY, TX_SUBMIT_RATE_WINDOW_SECS),
            trust_proxy_headers: false,
        };
        let fetch_limiter = RateLimitConfig {
            limiter: IpRateLimiter::new(FETCH_RATE_CAPACITY, FETCH_RATE_WINDOW_SECS),
            trust_proxy_headers: false,
        };
        let read_limiter = RateLimitConfig {
            limiter: IpRateLimiter::new(READ_RATE_CAPACITY, READ_RATE_WINDOW_SECS),
            trust_proxy_headers: false,
        };
        let sse_limiter = SseConnectionLimiter::new(SSE_MAX_CONNECTIONS);

        let router = Router::new()
            // ----------------------------------------------------------------
            // Unrestricted endpoints (health / ready / identity)
            // ----------------------------------------------------------------
            .route("/health", get(handlers::misc::health))
            .route("/ready", get(handlers::misc::ready))
            .route("/identity", get(handlers::misc::get_identity))
            .route("/wait-until-ready", get(handlers::misc::wait_until_ready))
            .route("/epoch-manager/stats", get(handlers::misc::get_epoch_manager_stats))
            .route("/network", get(handlers::network::get))
            .route("/network/stats", get(handlers::network::get_network_sync_stats))
            .route("/network/connections", get(handlers::network::get_connections))
            // ----------------------------------------------------------------
            // POST /substates/fetch – rate limited (fetch_limiter)
            // GET /substates/:id   – unrestricted
            // ----------------------------------------------------------------
            .nest("/substates", Router::new()
                .route("/fetch", post(handlers::substates::fetch_substates)
                    .route_layer(middleware::from_fn_with_state(
                        fetch_limiter.clone(),
                        rate_limit_middleware,
                    )))
                .route("/{substate_id}", get(handlers::substates::get_substate))
            )
            // ----------------------------------------------------------------
            // Transactions
            // ----------------------------------------------------------------
            .nest("/transactions", Router::new()
                // POST /transactions – 20 req/min per IP
                .route("/", post(handlers::transactions::submit_transaction)
                    .route_layer(middleware::from_fn_with_state(
                        tx_submit_limiter.clone(),
                        rate_limit_middleware,
                    )))
                // POST /transactions/dry-run – global tower rate limit (existing) +
                //   per-IP limit reusing tx_submit_limiter
                .route("/dry-run", post(handlers::transactions::submit_transaction_dry_run)
                    .route_layer(middleware::from_fn_with_state(
                        tx_submit_limiter.clone(),
                        rate_limit_middleware,
                    ))
                    .layer(ServiceBuilder::new()
                        .layer(axum::error_handling::HandleErrorLayer::new(|err: axum::BoxError| async move {
                            (
                                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Unhandled error: {}", err),
                            )
                        }))
                        .layer(BufferLayer::new(1024))
                        .layer(RateLimitLayer::new(10, Duration::from_secs(5)))
                    ))
                // GET /transactions/recent – rate limited (read_limiter)
                .route(
                    "/recent",
                    get(handlers::transactions::list_recent_transactions)
                        .route_layer(middleware::from_fn_with_state(
                            read_limiter.clone(),
                            rate_limit_middleware,
                        )),
                )
                .route(
                    "/{transaction_id}/result",
                    get(handlers::transactions::get_transaction_result),
                )
                .route("/events", get(handlers::transactions::query_transaction_events))
                // SSE stream – concurrent connection limit
                .route("/events/stream", get(handlers::transaction_events::sse_transaction_events)
                    .route_layer(middleware::from_fn_with_state(
                        sse_limiter.clone(),
                        sse_limit_middleware,
                    )))
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
            // GET /non-fungibles – rate limited (read_limiter)
            .route("/non-fungibles", get(handlers::nfts::get_non_fungibles)
                .route_layer(middleware::from_fn_with_state(
                    read_limiter.clone(),
                    rate_limit_middleware,
                )))
            .nest("/utxos", Router::new()
                .route("/", get(handlers::utxos::list_utxos))
                // POST /utxos/fetch – rate limited (fetch_limiter)
                .route("/fetch", post(handlers::utxos::fetch_utxos)
                    .route_layer(middleware::from_fn_with_state(
                        fetch_limiter.clone(),
                        rate_limit_middleware,
                    )))
                // POST /utxos/stream (SSE-like streaming) – concurrent connection limit
                .route("/stream", post(handlers::utxos::stream_utxo_updates)
                    .route_layer(middleware::from_fn_with_state(
                        sse_limiter.clone(),
                        sse_limit_middleware,
                    )))
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
            // SSE /events – concurrent connection limit
            .route("/events", get(handlers::indexer_events::sse_events)
                .route_layer(middleware::from_fn_with_state(
                    sse_limiter.clone(),
                    sse_limit_middleware,
                )))
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
            // `into_make_service_with_connect_info` populates `ConnectInfo<SocketAddr>`
            // for each connection, which the per-IP rate limiter middleware uses to
            // identify the remote peer when no proxy headers are present.
            if let Err(error) = axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(shutdown)
            .await
            {
                error!(target: LOG_TARGET, "Wallet query HTTP server error: {error}");
            }
        });

        Ok(listen_addr)
    }
}
