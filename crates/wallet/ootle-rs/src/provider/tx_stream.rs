//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{sync::Weak, time::Duration};

use futures::{Stream, StreamExt};
use tari_indexer_client::{
    error::IndexerRestClientError,
    rest_api_client::IndexerRestApiClient,
    sse,
    sse::SseStreamError,
};
use tokio::{sync::watch, time};
use tracing::{debug, error, trace};

#[derive(Debug, Clone)]
pub(crate) struct Paused {
    watch: watch::Sender<bool>,
}

impl Paused {
    /// Sets the paused state.
    /// Returns `true` if the state was changed, `false` if it was already set to the given value.
    pub(crate) fn set_paused(&self, paused: bool) -> bool {
        self.watch.send_if_modified(|v| {
            let prev_paused = *v;
            *v = paused;
            prev_paused != paused
        })
    }

    pub(crate) fn waiter(&self) -> PauseWaiter {
        PauseWaiter {
            rx: self.watch.subscribe(),
        }
    }
}

impl Default for Paused {
    fn default() -> Self {
        let (tx, _rx) = watch::channel(true);
        Self { watch: tx }
    }
}

pub(crate) struct PauseWaiter {
    rx: watch::Receiver<bool>,
}

impl PauseWaiter {
    pub(crate) fn is_paused(&self) -> bool {
        *self.rx.borrow()
    }

    /// Waits until the paused state is changed to `true`.
    ///
    /// Returns `true` if the method actually waited for the paused state to become paused,
    /// or `false` if it was already paused when called.
    pub(crate) async fn wait_paused(&mut self) -> bool {
        if self.is_paused() {
            return false;
        }

        if self.rx.changed().await.is_err() {
            return true;
        }
        debug_assert!(self.is_paused());
        true
    }

    /// Waits until the paused state is changed to `false`.
    ///
    /// Returns `true` if the method actually waited for the paused state to become unpaused,
    /// or `false` if it was already unpaused when called.
    pub(crate) async fn wait_unpaused(&mut self) -> bool {
        if !self.is_paused() {
            return false;
        }

        loop {
            if self.rx.changed().await.is_err() {
                return true;
            }
            if !self.is_paused() {
                break;
            }
        }
        true
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EventStreamError {
    #[error("Indexer REST client has been dropped")]
    ClientDropped,
    #[error("Indexer REST client error: {0}")]
    IndexerClientError(#[from] IndexerRestClientError),
    #[error("SSE stream error: {0}")]
    StreamError(#[from] SseStreamError),
}

pub struct EventStream {
    client: Weak<IndexerRestApiClient>,
    span: tracing::Span,
    paused: PauseWaiter,
}

impl EventStream {
    pub fn new(client: Weak<IndexerRestApiClient>, paused: PauseWaiter) -> Self {
        let span = tracing::debug_span!("EventStream");
        Self { client, span, paused }
    }

    pub fn into_stream(mut self) -> impl Stream<Item = Result<sse::Event, EventStreamError>> {
        async_stream::stream! {
            let client = match self.client.upgrade() {
                Some(client) => client,
                None => {
                    error!("Indexer REST client has been dropped");
                    yield Err(EventStreamError::ClientDropped);
                    return;
                },
            };
            loop {
                let _enter = self.span.enter();
                if self.paused.wait_unpaused().await {
                    debug!("event stream unpaused");
                }

                let mut events = match client.sse_events().await.map_err(EventStreamError::IndexerClientError) {
                    Ok(stream) => stream,
                    Err(err) => {
                        error!(%err, "failed to start event stream. Sleeping before retrying");
                        yield Err(err);
                        time::sleep(Duration::from_secs(5)).await;
                        continue;
                    },
                };

                loop {
                    tokio::select! {
                        _ = self.paused.wait_paused() => {
                            debug!("event stream paused");
                            break;
                        },
                        event = events.next() =>  {
                            match event {
                                Some(Ok(evt)) => {
                                    trace!(?evt, "received event");
                                    yield Ok(evt);
                                },
                                Some(Err(err)) => {
                                    error!(%err, "error receiving event");
                                    yield Err(EventStreamError::StreamError(err));
                                    break;
                                },
                                None => {
                                    debug!("event stream ended");
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
