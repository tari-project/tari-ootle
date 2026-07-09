//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{future, net::SocketAddr, sync::Arc, time::Instant};

use axum::{
    body::{Body, HttpBody},
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use log::*;
use prometheus_client::{
    encoding::text::encode,
    metrics::{
        counter::Counter,
        gauge::Gauge,
        histogram::{Histogram, exponential_buckets},
    },
    registry::Registry,
};
use tari_ootle_app_utilities::tcp::try_bind_with_fallback;
use tari_shutdown::ShutdownSignal;

use crate::metrics::CollectorRegister;

const LOG_TARGET: &str = "tari::ootle::indexer::rest_api::metrics";
const METRICS_CONTENT_TYPE: &str = "application/openmetrics-text;charset=utf-8;version=1.0.0";
#[derive(Debug, Clone)]
pub struct MetricsHandler(Arc<Registry>);

impl MetricsHandler {
    pub fn new(registry: Registry) -> Self {
        Self(Arc::new(registry))
    }
}

impl<S> axum::handler::Handler<(), S> for MetricsHandler {
    type Future = future::Ready<Response>;

    fn call(self, req: Request<Body>, _state: S) -> Self::Future {
        if req.method() != axum::http::Method::GET {
            let mut resp = "Method not allowed. Only GET requests are supported for metrics.".into_response();
            *resp.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
            return future::ready(resp);
        }

        let mut text = String::with_capacity(1024);
        encode(&mut text, &self.0).unwrap();

        future::ready(
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, METRICS_CONTENT_TYPE)],
                text,
            )
                .into_response(),
        )
    }
}

pub async fn spawn_server(
    preferred_addr: SocketAddr,
    registry: Registry,
    shutdown: ShutdownSignal,
) -> anyhow::Result<SocketAddr> {
    let listener = try_bind_with_fallback(preferred_addr).await?;
    let listen_addr = listener.local_addr()?;
    let router = Router::new().route("/_metrics", get(MetricsHandler::new(registry)));

    info!(target: LOG_TARGET, "Indexer metrics server listening on {listen_addr}");
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router).with_graceful_shutdown(shutdown).await {
            error!(target: LOG_TARGET, "Indexer metrics HTTP server error: {error}");
        }
    });

    Ok(listen_addr)
}

#[derive(Clone)]
pub struct RequestMetrics {
    request_counter: Counter,
    response_time_histogram: Histogram,
    requests_pending: Gauge,
    response_body_size_histogram: Histogram,
}

pub fn register(registry: &mut Registry) -> RequestMetrics {
    let registry = registry.sub_registry_with_prefix("api");

    RequestMetrics {
        request_counter: Counter::default().register_at(
            "http_requests_total",
            "Total number of HTTP requests received",
            registry,
        ),
        response_time_histogram: Histogram::new(
            exponential_buckets(0.001, 2.0, 15), // buckets from 1ms, doubling, 15 buckets
        )
        .register_at("http_response_time_seconds", "HTTP response times in seconds", registry),
        requests_pending: Gauge::default().register_at(
            "http_requests_pending",
            "Number of HTTP requests currently being processed",
            registry,
        ),
        response_body_size_histogram: Histogram::new(
            exponential_buckets(100.0, 2.0, 15), // buckets from 100B, doubling, 15 buckets
        ),
    }
}

pub async fn layer(State(metrics): State<RequestMetrics>, req: Request<Body>, next: Next) -> Response {
    metrics.request_counter.inc();
    metrics.requests_pending.inc();

    let timer = Instant::now();
    let response = next.run(req).await;
    if let Some(size) = response.size_hint().exact() {
        metrics.response_body_size_histogram.observe(size as f64);
    }
    let elapsed = timer.elapsed().as_secs_f64();
    metrics.response_time_histogram.observe(elapsed);
    metrics.requests_pending.dec();

    response
}
