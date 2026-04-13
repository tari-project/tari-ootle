//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::net::SocketAddr;

use axum::{
    Extension,
    extract::ConnectInfo,
    http::StatusCode,
    response::{IntoResponse, Response, Sse, sse},
};
use futures::Stream;
use log::*;
use tari_indexer_client::event::IndexerEvent;
use tokio_stream::StreamExt;

use crate::rest_api::context::HandlerContext;

const LOG_TARGET: &str = "tari::indexer::rest_api::handlers::events";

#[utoipa::path(get, path = "/events", description = "SSE events")]
pub async fn sse_events(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Extension(context): Extension<HandlerContext>,
) -> Response {
    // Check connection limit
    let _guard = match context.sse_connection_limiter.try_acquire(addr.ip()) {
        Ok(guard) => guard,
        Err(()) => {
            warn!(target: LOG_TARGET, "SSE connection limit exceeded for IP: {}", addr.ip());
            let mut response = (
                StatusCode::TOO_MANY_REQUESTS,
                "Too many concurrent SSE connections from this IP",
            )
                .into_response();
            // Add Retry-After header suggesting to retry in 60 seconds
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from_static("60"),
            );
            return response;
        },
    };

    info!(target: LOG_TARGET, "Client connected to SSE event stream");
    let event_stream = tokio_stream::wrappers::BroadcastStream::new(context.subscribe_events())
        .take_while(|res| res.is_ok())
        .map(|res| res.expect("take_while should prevent errors here"))
        .map(move |event| {
            let _ = &_guard; // Keep guard alive
            encode_event(&event)
        });

    Sse::new(event_stream).keep_alive(sse::KeepAlive::new()).into_response()
}

fn encode_event(event: &IndexerEvent) -> Result<sse::Event, axum::Error> {
    let encoded = sse::Event::default().event(event.as_event_name());
    match event {
        IndexerEvent::NewEpoch(event) => encoded.json_data(event),
        IndexerEvent::TransactionFinalized(event) => encoded.json_data(event),
    }
}
