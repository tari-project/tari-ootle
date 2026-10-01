//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::pin::Pin;

use axum::{
    Extension,
    extract::Query,
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response, Sse, sse},
};
use futures::Stream;
use log::*;
use tari_engine_types::substate::SubstateId;
use tari_indexer_client::{event::TransactionEvent, types::StreamTransactionEventsRequest};
use tari_ootle_transaction::TransactionId;
use tari_template_lib_types::{Metadata, TemplateAddress};
use tokio_stream::StreamExt;

use crate::{
    event_manager::WILDCARD_TOPIC_SCAN_LIMIT,
    network_state_sync::EventFilter,
    rest_api::{context::HandlerContext, handlers::HandlerResult, streaming::disable_proxy_buffering},
    storage_sqlite::SqliteIndexerStore,
    store::{EventQuery, ReadOnlyStore},
};

const LOG_TARGET: &str = "tari::indexer::rest_api::handlers::transaction_events";

/// Maximum number of events that can be replayed from the database on reconnect.
const MAX_REPLAY_EVENTS: u32 = 10_000;
/// Page size for DB replay queries.
const REPLAY_PAGE_SIZE: u32 = 500;
/// Pages one catch-up may read. A wildcard page can examine `WILDCARD_TOPIC_SCAN_LIMIT` rows and
/// deliver none, so the budget counts pages rather than delivered events.
const MAX_REPLAY_PAGES: u32 = MAX_REPLAY_EVENTS / REPLAY_PAGE_SIZE;

#[utoipa::path(
    get,
    path = "/transactions/events/stream",
    description = "SSE stream of template-emitted transaction events. Supports catch-up via \
                    the `after_id` query parameter or `Last-Event-ID` header.",
    params(
        ("topic" = Option<String>, Query, description = "Filter by event topic"),
        ("substate_id" = Option<String>, Query, description = "Filter by substate ID"),
        ("template_address" = Option<String>, Query, description = "Filter by template address"),
        ("resource_address" = Option<String>, Query, description = "Filter by resource address. \
            Matches only std.resource.* events, which carry the resource as their substate_id. \
            Vault events name no resource: filter those by substate_id (the vault ID)"),
        ("after_id" = Option<i64>, Query, description = "Resume from this event ID (exclusive)"),
    )
)]
pub async fn sse_transaction_events(
    Extension(context): Extension<HandlerContext>,
    headers: HeaderMap,
    Query(req): Query<StreamTransactionEventsRequest>,
) -> HandlerResult<Response> {
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
        resource_address: req.resource_address,
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
            Box::pin(replay_then_live_stream(store, broadcast_rx, filter, after_id))
        },
        None => Box::pin(live_only_stream(broadcast_rx, filter)),
    };

    let mut response = Sse::new(event_stream).keep_alive(sse::KeepAlive::new()).into_response();
    // `Last-Event-ID` selects where the stream resumes from, so the same URL does not describe the
    // same response.
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("last-event-id"));
    disable_proxy_buffering(response.headers_mut());
    Ok(response)
}

/// Stream that only forwards live broadcast events (no replay).
/// Lagged events are silently skipped (the client has no after_id so there's nothing to replay).
fn live_only_stream(
    broadcast_rx: tokio::sync::broadcast::Receiver<TransactionEvent>,
    filter: EventFilter,
) -> impl Stream<Item = Result<sse::Event, axum::Error>> {
    tokio_stream::wrappers::BroadcastStream::new(broadcast_rx).filter_map(move |res| match res {
        Ok(tx_event) if filter.matches(&tx_event.event) => Some(encode_transaction_event(&tx_event)),
        Ok(_) => None,
        // Lagged: events were dropped from the broadcast buffer, skip and continue
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_)) => {
            warn!(target: LOG_TARGET, "Live-only SSE client lagged, some events were dropped");
            None
        },
    })
}

/// Stream that first replays missed events from the DB, then switches to live.
/// Events are deduplicated during the transition using the event ID.
fn replay_then_live_stream(
    store: ReadOnlyStore<SqliteIndexerStore>,
    broadcast_rx: tokio::sync::broadcast::Receiver<TransactionEvent>,
    filter: EventFilter,
    after_id: i64,
) -> impl Stream<Item = Result<sse::Event, axum::Error>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<sse::Event, axum::Error>>(256);

    tokio::spawn(async move {
        let result = run_replay_then_live(store, broadcast_rx, filter, after_id, &tx).await;
        if let Err(e) = result {
            warn!(target: LOG_TARGET, "SSE replay-then-live stream error: {}", e);
        }
        // tx is dropped here, which closes the stream
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
    let query = replay_query(&filter);
    let Some(mut highest_id) = replay_stored(&store, &query, after_id, tx).await? else {
        // Client disconnected
        return Ok(());
    };

    debug!(target: LOG_TARGET, "SSE replay complete (highest_id={})", highest_id);

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
                match replay_stored(&store, &query, highest_id, tx).await? {
                    Some(caught_up_to) => highest_id = caught_up_to,
                    None => return Ok(()),
                }
            },
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                return Ok(());
            },
        }
    }
}

/// Sends the stored events matching `query` with id above `after_id`, reading at most
/// `MAX_REPLAY_PAGES` pages. Returns the id the client has been brought up to, or `None` if the
/// client disconnected.
async fn replay_stored(
    store: &ReadOnlyStore<SqliteIndexerStore>,
    query: &EventQuery,
    after_id: i64,
    tx: &tokio::sync::mpsc::Sender<Result<sse::Event, axum::Error>>,
) -> Result<Option<i64>, anyhow::Error> {
    let mut highest_id = after_id;
    for _ in 0..MAX_REPLAY_PAGES {
        let page = store
            .get_events_after_id(query.clone(), highest_id, REPLAY_PAGE_SIZE)
            .await?;

        for (id, transaction_id, event) in &page.events {
            highest_id = *id;
            let sse_event = encode_replay_event(*id, transaction_id, event);
            if tx.send(sse_event).await.is_err() {
                return Ok(None);
            }
        }

        match page.next_cursor {
            Some(cursor) => highest_id = cursor,
            None => break,
        }
    }
    Ok(Some(highest_id))
}

fn replay_query(filter: &EventFilter) -> EventQuery {
    EventQuery {
        topic: filter.topic.as_deref().map(str::to_owned),
        substate_id: filter.substate_id.clone(),
        template_address: filter.template_address,
        resource_address: filter.resource_address,
        wildcard_scan_limit: WILDCARD_TOPIC_SCAN_LIMIT,
    }
}

/// Encode a live TransactionEvent (which already carries its DB id) as an SSE event.
/// The SSE event type is set to the event topic (e.g. "std.vault.withdraw").
fn encode_transaction_event(event: &TransactionEvent) -> Result<sse::Event, axum::Error> {
    sse::Event::default()
        .event(event.event.topic())
        .id(event.id.to_string())
        .json_data(TransactionEventMinimal::from(event))
}

/// Encode a replayed event from the DB as an SSE event.
fn encode_replay_event(
    id: i64,
    transaction_id: &TransactionId,
    event: &tari_engine_types::events::Event,
) -> Result<sse::Event, axum::Error> {
    let tx_event = TransactionEventMinimal {
        transaction_id,
        event: event.into(),
    };
    sse::Event::default()
        .event(event.topic())
        .id(id.to_string())
        .json_data(&tx_event)
}

#[derive(serde::Serialize)]
struct TransactionEventMinimal<'a> {
    pub transaction_id: &'a TransactionId,
    pub event: EventMinimal<'a>,
}

impl<'a> From<&'a TransactionEvent> for TransactionEventMinimal<'a> {
    fn from(tx_event: &'a TransactionEvent) -> Self {
        let event = &*tx_event.event;
        Self {
            transaction_id: &tx_event.transaction_id,
            event: event.into(),
        }
    }
}

#[derive(serde::Serialize)]
struct EventMinimal<'a> {
    substate_id: Option<&'a SubstateId>,
    template_address: &'a TemplateAddress,
    payload: &'a Metadata,
    // no topic since it is already emitted in the SSE event
}

impl<'a> From<&'a tari_engine_types::events::Event> for EventMinimal<'a> {
    fn from(event: &'a tari_engine_types::events::Event) -> Self {
        Self {
            substate_id: event.substate_id(),
            template_address: event.template_address(),
            payload: event.payload(),
        }
    }
}
