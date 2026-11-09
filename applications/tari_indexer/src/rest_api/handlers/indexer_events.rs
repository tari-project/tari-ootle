//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{
    Extension,
    response::{IntoResponse, Response, Sse, sse},
};
use futures::Stream;
use log::{info, warn};
use tari_indexer_client::event::IndexerEvent;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;

use crate::rest_api::{context::HandlerContext, handlers::HandlerResult, streaming::disable_proxy_buffering};

const LOG_TARGET: &str = "tari::indexer::rest_api::handlers::events";

#[utoipa::path(get, path = "/events", description = "SSE events")]
pub async fn sse_events(Extension(context): Extension<HandlerContext>) -> HandlerResult<Response> {
    info!(target: LOG_TARGET, "Client connected to SSE event stream");
    let event_stream = event_stream(context.subscribe_events());

    let mut response = Sse::new(event_stream).keep_alive(sse::KeepAlive::new()).into_response();
    disable_proxy_buffering(response.headers_mut());
    Ok(response)
}

fn event_stream(rx: broadcast::Receiver<IndexerEvent>) -> impl Stream<Item = Result<sse::Event, axum::Error>> {
    tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(|res| match res {
        Ok(event) => Some(encode_event(&event)),
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
            warn!(target: LOG_TARGET, "SSE event stream client lagged by {n} events, some events were dropped");
            None
        },
    })
}

fn encode_event(event: &IndexerEvent) -> Result<sse::Event, axum::Error> {
    let encoded = sse::Event::default().event(event.as_event_name());
    match event {
        IndexerEvent::NewEpoch(event) => encoded.json_data(event),
        IndexerEvent::TransactionFinalized(event) => encoded.json_data(event),
    }
}

#[cfg(test)]
mod tests {
    use tari_indexer_client::event::NewEpochEvent;
    use tari_ootle_common_types::Epoch;

    use super::*;

    #[tokio::test]
    async fn sse_event_stream_continues_after_broadcast_lag() {
        let (tx, rx) = broadcast::channel(2);
        let mut stream = Box::pin(event_stream(rx));

        for epoch in 1..=4 {
            tx.send(IndexerEvent::NewEpoch(NewEpochEvent { epoch: Epoch(epoch) }))
                .unwrap();
        }

        let first = stream
            .next()
            .await
            .expect("stream should stay open after lag")
            .expect("SSE event encoding should succeed");
        let second = stream
            .next()
            .await
            .expect("stream should yield remaining buffered event")
            .expect("SSE event encoding should succeed");

        assert!(format!("{first:?}").contains("\"epoch\":3"), "{first:?}");
        assert!(format!("{second:?}").contains("\"epoch\":4"), "{second:?}");
    }
}
