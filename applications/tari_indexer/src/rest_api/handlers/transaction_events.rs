//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{
    Extension,
    extract::Query,
    response::{Sse, sse},
};
use futures::Stream;
use log::info;
use tari_indexer_client::{event::TransactionEvent, types::StreamTransactionEventsRequest};
use tokio_stream::StreamExt;

use crate::{
    network_state_sync::EventFilter,
    rest_api::{context::HandlerContext, handlers::HandlerResult},
};

const LOG_TARGET: &str = "tari::indexer::rest_api::handlers::transaction_events";

#[utoipa::path(
    get,
    path = "/transactions/events/stream",
    description = "SSE stream of template-emitted transaction events",
    params(
        ("topic" = Option<String>, Query, description = "Filter by event topic"),
        ("substate_id" = Option<String>, Query, description = "Filter by substate ID"),
        ("template_address" = Option<String>, Query, description = "Filter by template address"),
    )
)]
pub async fn sse_transaction_events(
    Extension(context): Extension<HandlerContext>,
    Query(req): Query<StreamTransactionEventsRequest>,
) -> HandlerResult<Sse<impl Stream<Item = Result<sse::Event, axum::Error>>>> {
    info!(target: LOG_TARGET, "Client connected to transaction events SSE stream");

    let filter = EventFilter {
        topic: req.topic.map(|s| s.into_boxed_str()),
        entity_id: None,
        substate_id: req.substate_id,
        template_address: req.template_address,
    };

    let event_stream = tokio_stream::wrappers::BroadcastStream::new(context.subscribe_transaction_events())
        .take_while(|res| res.is_ok())
        .map(|res| res.expect("take_while should prevent errors here"))
        .filter(move |tx_event| filter.matches(&tx_event.event))
        .map(|tx_event| encode_transaction_event(&tx_event));

    Ok(Sse::new(event_stream).keep_alive(sse::KeepAlive::new()))
}

fn encode_transaction_event(event: &TransactionEvent) -> Result<sse::Event, axum::Error> {
    sse::Event::default().event("TransactionEvent").json_data(event)
}
