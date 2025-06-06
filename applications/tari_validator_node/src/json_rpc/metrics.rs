//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{future, future::Future, pin::Pin, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use prometheus_client::{encoding::text::encode, registry::Registry};
use tokio::sync::Mutex;

const METRICS_CONTENT_TYPE: &str = "application/openmetrics-text;charset=utf-8;version=1.0.0";
#[derive(Debug, Clone)]
pub struct MetricsHandler(Arc<Mutex<Registry>>);

impl MetricsHandler {
    pub fn new(registry: Registry) -> Self {
        Self(Arc::new(Mutex::new(registry)))
    }
}

impl<S> axum::handler::Handler<(), S> for MetricsHandler {
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request<Body>, _state: S) -> Self::Future {
        if req.method() != axum::http::Method::GET {
            let mut resp = "Method not allowed. Only GET requests are supported for metrics.".into_response();
            *resp.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
            return Box::pin(future::ready(resp));
        }

        Box::pin(async move {
            let reg = self.0.lock().await;
            let mut text = String::with_capacity(1024);
            encode(&mut text, &reg).unwrap();

            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, METRICS_CONTENT_TYPE)],
                text,
            )
                .into_response()
        })
    }
}
