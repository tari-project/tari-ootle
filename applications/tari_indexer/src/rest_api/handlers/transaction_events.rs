//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{net::SocketAddr, pin::Pin, sync::Arc};

use axum::{
    Extension,
    extract::{ConnectInfo, Query},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response, Sse, sse},
};
use futures::Stream;
use log::*;
use tari_indexer_client::{event::TransactionEvent, types::StreamTransactionEventsRequest};
use tokio_stream::StreamExt;

use crate::{
    network_state_sync::EventFilter,
    rest_api::{context::HandlerContext, handlers::HandlerResult},
    storage_sqlite::SqliteIndexerStore,
    store::ReadOnlyStore,
};

const LOG_TARGET: &str = "tari::indexer::rest_api::handlers::transaction_events";

/// Maximum number of events that can be replayed from the database on reconnect.
const MAX_REPLAY_EVENTS: u32 = 10_000;
/// Page size for DB replay queries.
const REPLAY_PAGE_SIZE: u32 = 500;

#[utoipa::path(
    get,
    path = "/transactions/events/stream",
    description = "SSE stream of template-emitted transaction events. Supports catch-up via \
                    the `after_id` query parameter or `Last-Event-ID` header.",
    params(
        ("topic" = Option<String>, Query, description = "Filter by event topic"),
        ("substate_id" = Option<String>, Query, description = "Filter by substate ID"),
        ("template_address" = Option<String>, Query, description = "Filter by template address"),
        ("after_id" = Option<i64>, Query, description = "Resume from this event ID (exclusive)"),
    )
)]
pub async fn sse_transaction_events(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Extension(context): Extension<HandlerContext>,
    headers: HeaderMap,
    Query(req): Query<StreamTransactionEventsRequest>,
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

    // Resolve after_id: prefer Last-Event-ID header (SSE spec), fall back to query param
    let after_id = headers
        .get("Last-Event-ID")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
        .or(req.after_id);

    let filter = EventFilter {
        topic: req.topic.map(|s| s.into_boxed_str()),
        entity_id: None,
        substate_id: req.substate_id,
        template_address: req.template_address,
    };

    if let Some(id) = after_id {
        info!(target: LOG_TARGET, "Client connected to transaction events SSE stream (catch-up from id={})", id);
    } else {
        info!(target: LOG_TARGET, "Client connected to transaction events SSE stream (live)");
    }

    // Subscribe to the broadcast channel BEFORE reading the DB.
    // This ensures no events are missed between the DB read and the live stream.
    let broadcast_rx = context.subscribe_transaction_events();

    type SseStream = Pin<Box<dyn Stream<Item = Result<sse::Event, axum::Error>> + Send>>;

    let event_stream: SseStream = match after_id {
        Some(after_id) => {
            let store = context.read_only_store().clone();
            Box::pin(replay_then_live_stream(store, broadcast_rx, filter, after_id, _guard))
        },
        None => Box::pin(live_only_stream(broadcast_rx, filter, _guard)),
    };

    Sse::new(event_stream).keep_alive(sse::KeepAlive::new()).into_response()
}

/// Stream that only forwards live broadcast events (no replay).
/// Lagged events are silently skipped (the client has no after_id so there's nothing to replay).
fn live_only_stream(
    broadcast_rx: tokio::sync::broadcast::Receiver<TransactionEvent>,
    filter: EventFilter,
    _guard: crate::rest_api::rate_limit::SseConnectionGuard,
) -> impl Stream<Item = Result<sse::Event, axum::Error>> {
    tokio_stream::wrappers::BroadcastStream::new(broadcast_rx).filter_map(move |res| {
        let _ = &_guard; // Keep guard alive
        match res {
            Ok(tx_event) if filter.matches(&tx_event.event) => Some(encode_transaction_event(&tx_event)),
            Ok(_) => None,
            // Lagged: events were dropped from the broadcast buffer, skip and continue
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_)) => {
                warn!(target: LOG_TARGET, "Live-only SSE client lagged, some events were dropped");
                None
            },
        }
    })
}

/// Stream that first replays missed events from the DB, then switches to live.
/// Events are deduplicated during the transition using the event ID.
fn replay_then_live_stream(
    store: ReadOnlyStore<SqliteIndexerStore>,
    broadcast_rx: tokio::sync::broadcast::Receiver<TransactionEvent>,
    filter: EventFilter,
    after_id: i64,
    _guard: crate::rest_api::rate_limit::SseConnectionGuard,
) -> impl Stream<Item = Result<sse::Event, axum::Error>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<sse::Event, axum::Error>>(256);

    tokio::spawn(async move {
        let _guard = _guard; // Move guard into task
        let result = run_replay_then_live(store, broadcast_rx, filter, after_id, &tx).await;
        if let Err(e) = result {
            warn!(target: LOG_TARGET, "SSE replay-then-live stream error: {}", e);
        }
        // tx and _guard are dropped here, which closes the stream and releases the connection
    });

    tokio_stream::wrappers::ReceiverStream::new(rx)
}

async fn run_replay_then_live(
    store: ReadOnlyStore<SqliteIndexerStore>,
    mut broadcast_rx: tokio::sync::broadcast::Receiver<TransactionEvent>,
    filter: EventFilter,
    after_id: i64,
    tx: &tokio::sync::mpsc::Sender<Result<sse::Event, axum::Error>>,
) -> Result<(), anyhow::Error> {
    // Phase 1: Replay from DB
    let mut highest_id = after_id;
    let mut total_replayed = 0u32;

    loop {
        let batch = store.get_events_after_id(
            highest_id,
            filter.topic.as_deref(),
            filter.substate_id.as_ref(),
            filter.template_address.as_ref(),
            REPLAY_PAGE_SIZE,
        )?;

        if batch.is_empty() {
            break;
        }

        for (id, transaction_id, event) in &batch {
            highest_id = *id;
            total_replayed += 1;

            let sse_event = encode_replay_event(*id, transaction_id, event);
            if tx.send(sse_event).await.is_err() {
                // Client disconnected
                return Ok(());
            }
        }

        if batch.len() < REPLAY_PAGE_SIZE as usize || total_replayed >= MAX_REPLAY_EVENTS {
            break;
        }
    }

    debug!(target: LOG_TARGET, "SSE replay complete: {} events replayed (highest_id={})", total_replayed, highest_id);

    // Phase 2: Live stream with dedup
    loop {
        match broadcast_rx.recv().await {
            Ok(tx_event) => {
                // Skip events we already replayed
                if tx_event.id <= highest_id {
                    continue;
                }
                if !filter.matches(&tx_event.event) {
                    continue;
                }
                highest_id = tx_event.id;

                let sse_event = encode_transaction_event(&tx_event);
                if tx.send(sse_event).await.is_err() {
                    return Ok(());
                }
            },
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                warn!(target: LOG_TARGET, "SSE broadcast lagged by {} events, catching up from DB (highest_id={})", n, highest_id);
                // Iteratively catch up from DB in pages until we've replayed everything
                loop {
                    let batch = store.get_events_after_id(
                        highest_id,
                        filter.topic.as_deref(),
                        filter.substate_id.as_ref(),
                        filter.template_address.as_ref(),
                        REPLAY_PAGE_SIZE,
                    )?;

                    if batch.is_empty() {
                        break;
                    }

                    for (id, transaction_id, event) in &batch {
                        highest_id = *id;
                        let sse_event = encode_replay_event(*id, transaction_id, event);
                        if tx.send(sse_event).await.is_err() {
                            return Ok(());
                        }
                    }

                    if batch.len() < REPLAY_PAGE_SIZE as usize {
                        break;
                    }
                }
            },
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                return Ok(());
            },
        }
    }
}

/// Encode a live TransactionEvent (which already carries its DB id) as an SSE event.
/// The SSE event type is set to the event topic (e.g. "std.vault.withdraw").
fn encode_transaction_event(event: &TransactionEvent) -> Result<sse::Event, axum::Error> {
    sse::Event::default()
        .event(event.event.topic())
        .id(event.id.to_string())
        .json_data(event)
}

/// Encode a replayed event from the DB as an SSE event.
fn encode_replay_event(
    id: i64,
    transaction_id: &tari_ootle_transaction::TransactionId,
    event: &tari_engine_types::events::Event,
) -> Result<sse::Event, axum::Error> {
    let tx_event = TransactionEvent {
        id,
        transaction_id: *transaction_id,
        event: Arc::new(event.clone()),
    };
    sse::Event::default()
        .event(event.topic())
        .id(id.to_string())
        .json_data(&tx_event)
}
