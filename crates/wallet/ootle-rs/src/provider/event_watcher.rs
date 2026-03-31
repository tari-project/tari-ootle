//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Weak;

use futures::{Stream, StreamExt};
use tari_indexer_client::{
    error::IndexerRestClientError,
    event::TransactionEvent,
    rest_api_client::IndexerRestApiClient,
    sse::SseStreamError,
    types::StreamTransactionEventsRequest,
};
use tari_ootle_common_types::engine_types::substate::SubstateId;
use tari_template_lib_types::TemplateAddress;
use tracing::error;

/// Filter for subscribing to transaction events via SSE.
#[derive(Debug, Clone, Default)]
pub struct TransactionEventFilter {
    pub topic: Option<String>,
    pub substate_id: Option<SubstateId>,
    pub template_address: Option<TemplateAddress>,
    /// Resume the event stream from this event ID (exclusive).
    /// Events with id > after_id will be replayed from the database before switching to live.
    pub after_id: Option<i64>,
}

impl TransactionEventFilter {
    fn into_request(self) -> StreamTransactionEventsRequest {
        StreamTransactionEventsRequest {
            topic: self.topic,
            substate_id: self.substate_id,
            template_address: self.template_address,
            after_id: self.after_id,
        }
    }
}

/// Error type for the transaction event stream.
#[derive(Debug, thiserror::Error)]
pub enum EventWatcherError {
    #[error("Indexer REST client has been dropped")]
    ClientDropped,
    #[error("Indexer REST client error: {0}")]
    IndexerClientError(#[from] IndexerRestClientError),
    #[error("SSE stream error: {0}")]
    StreamError(#[from] SseStreamError),
    #[error("Failed to parse transaction event: {0}")]
    ParseError(#[from] serde_json::Error),
}

/// A stream of transaction events from the indexer.
/// Created via `IndexerProvider::watch_events()`.
pub struct TransactionEventStream {
    client: Weak<IndexerRestApiClient>,
    filter: TransactionEventFilter,
}

impl TransactionEventStream {
    pub(crate) fn new(client: Weak<IndexerRestApiClient>, filter: TransactionEventFilter) -> Self {
        Self { client, filter }
    }

    /// Consume into a futures Stream of TransactionEvent items.
    pub fn into_stream(self) -> impl Stream<Item = Result<TransactionEvent, EventWatcherError>> {
        async_stream::stream! {
            let client = match self.client.upgrade() {
                Some(client) => client,
                None => {
                    error!("Indexer REST client has been dropped");
                    yield Err(EventWatcherError::ClientDropped);
                    return;
                },
            };

            let req = self.filter.into_request();
            let mut events = match client.sse_transaction_events(req).await {
                Ok(stream) => stream,
                Err(err) => {
                    error!(%err, "Failed to start transaction event stream");
                    yield Err(EventWatcherError::IndexerClientError(err));
                    return;
                },
            };

            const TX_EVENT_TYPE: &str = "TransactionEvent";

            loop {
                match events.next().await {
                    Some(Ok(evt)) => {
                        if evt.event_type != TX_EVENT_TYPE {
                            continue;
                        }
                        match evt.try_parse_event::<TransactionEvent>() {
                            Ok(tx_event) => yield Ok(tx_event),
                            Err(err) => {
                                error!(%err, "Failed to parse transaction event");
                                yield Err(EventWatcherError::ParseError(err));
                                return;
                            },
                        }
                    },
                    Some(Err(err)) => {
                        error!(%err, "Error receiving transaction event");
                        yield Err(EventWatcherError::StreamError(err));
                        return;
                    },
                    None => {
                        return;
                    },
                }
            }
        }
    }
}
