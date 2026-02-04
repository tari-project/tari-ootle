//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{future, sync::Arc, time::Instant};

use axum::{
    body::{Body, HttpBody},
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use prometheus_client::{
    encoding::text::encode,
    metrics::{
        counter::Counter,
        gauge::Gauge,
        histogram::{Histogram, exponential_buckets},
    },
    registry::Registry,
};

use crate::metrics::CollectorRegister;

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
