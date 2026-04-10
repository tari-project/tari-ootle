//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{net::SocketAddr, time::Duration};

use axum::{
    Extension,
    Router,
    body::Body,
    extract::{ConnectInfo, Request},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::Response,
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
    rest_api::{context::HandlerContext, handlers, rate_limit::IpRateLimiter},
};

const LOG_TARGET: &str = "tari::ootle::indexer::rest_api::server";

// Limit the body size to 4MB to allow for large transactions (wasm uploads)
const REQUEST_BODY_LIMIT: usize = 4 * 1024 * 1024; // 4 MB

/// Middleware for per-IP rate limiting
async fn rate_limit_middleware(
    limiter: IpRateLimiter,
) -> impl Fn(ConnectInfo<SocketAddr>, Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> + Clone {
    move |ConnectInfo(addr): ConnectInfo<SocketAddr>, request: Request, next: Next| {
        let limiter = limiter.clone();
        Box::pin(async move {
            match limiter.check_rate_limit(addr.ip()) {
                Ok(()) => next.run(request).await,
                Err(retry_after) => {
                    let mut response = Response::new(Body::from("Too Many Requests"));
                    *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
                    response.headers_mut().insert(
                        header::RETRY_AFTER,
                        HeaderValue::from_str(&retry_after.as_secs().to_string()).unwrap(),
                    );
                    response
                },
            }
        })
    }
}

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
                        let limiter = context.fetch_substates_limiter.clone();
                        move |addr, req, next| rate_limit_middleware(limiter.clone())(addr, req, next)
                    })))
                .route("/{substate_id}", get(handlers::substates::get_substate))
            )
            .nest("/transactions", Router::new()
                .route("/", post(handlers::transactions::submit_transaction)
                    .layer(middleware::from_fn({
                        let limiter = context.submit_transaction_limiter.clone();
                        move |addr, req, next| rate_limit_middleware(limiter.clone())(addr, req, next)
                    })))
                .route("/dry-run", post(handlers::transactions::submit_transaction_dry_run)
                    .layer(ServiceBuilder::new()
                        .layer(axum::error_handling::HandleErrorLayer::new(|err: axum::BoxError| async move {
                            (
                                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Unhandled error: {}", err),
                            )
                        }))
                        .layer(BufferLayer::new(1024))
                        .layer(RateLimitLayer::new(10, Duration::from_secs(5)))
                    )
                    .layer(middleware::from_fn({
                        let limiter = context.dry_run_limiter.clone();
                        move |addr, req, next| rate_limit_middleware(limiter.clone())(addr, req, next)
                    })))
                .route(
                    "/recent",
                    get(handlers::transactions::list_recent_transactions)
                        .layer(middleware::from_fn({
                            let limiter = context.list_recent_transactions_limiter.clone();
                            move |addr, req, next| rate_limit_middleware(limiter.clone())(addr, req, next)
                        })),
                )
                .route(
                    "/{transaction_id}/result",
                    get(handlers::transactions::get_transaction_result),
                )
                .route("/events", get(handlers::transactions::query_transaction_events))
                .route("/events/stream", get(handlers::transaction_events::sse_transaction_events))
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
                .layer(middleware::from_fn({
                    let limiter = context.get_non_fungibles_limiter.clone();
                    move |addr, req, next| rate_limit_middleware(limiter.clone())(addr, req, next)
                })))
            .nest("/utxos", Router::new()
                .route("/", get(handlers::utxos::list_utxos))
                .route("/fetch", post(handlers::utxos::fetch_utxos)
                    .layer(middleware::from_fn({
                        let limiter = context.fetch_utxos_limiter.clone();
                        move |addr, req, next| rate_limit_middleware(limiter.clone())(addr, req, next)
                    })))
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
            .nest("/resources/", Router::new()
                // Convenience Shortcut
                .route("/xtr" , get(handlers::resources::get_tari))
                .route("/tari" , get(handlers::resources::get_tari))
                .route("/{resource_address}" , get(handlers::resources::get_resource)))
            .route("/events", get(handlers::indexer_events::sse_events))
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
