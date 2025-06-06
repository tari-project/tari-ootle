//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::sync::watch;

use crate::configured::{EpochTicker, EpochTickerData};

pub fn watch_ticker() -> (WatchEpochTicker, watch::Sender<EpochTickerData>) {
    let (tx, mut rx) = watch::channel(EpochTickerData::default());
    // Dont want the dummy epoch 0 data
    rx.mark_unchanged();
    (WatchEpochTicker::new(rx), tx)
}

type TickerFuture = Pin<Box<dyn Future<Output = Option<watch::Receiver<EpochTickerData>>> + Send>>;

pub struct WatchEpochTicker {
    watch: Option<watch::Receiver<EpochTickerData>>,
    ticker_fut: Option<TickerFuture>,
}

impl WatchEpochTicker {
    pub fn new(watch: watch::Receiver<EpochTickerData>) -> Self {
        Self {
            watch: Some(watch),
            ticker_fut: None,
        }
    }
}

impl EpochTicker for WatchEpochTicker {
    fn poll_tick(&mut self, cx: &mut Context) -> Poll<Option<EpochTickerData>> {
        if self.watch.is_none() && self.ticker_fut.is_none() {
            // Polling after completion
            return Poll::Pending;
        }

        loop {
            if self.ticker_fut.is_none() {
                // Create a future that will resolve when the watch changes
                // NOTE that you cannot clone the watch receiver, because only the cloned watch will be marked as seen.
                let mut watch = self.watch.take().expect("watch receiver is None");
                self.ticker_fut = Some(Box::pin(async move {
                    if watch.changed().await.is_ok() {
                        Some(watch)
                    } else {
                        None
                    }
                }));
            }
            if let Some(fut) = self.ticker_fut.as_mut() {
                // Poll the future to see if it has resolved
                match fut.as_mut().poll(cx) {
                    Poll::Ready(Some(mut watch)) => {
                        // Read the latest value from the watch receiver and mark it as seen
                        let epoch_data = watch.borrow_and_update().clone();
                        self.ticker_fut = None; // Reset the future after polling
                        self.watch = Some(watch);
                        return Poll::Ready(Some(epoch_data));
                    },
                    Poll::Ready(None) => {
                        // The watch receiver has been dropped, so we can stop polling
                        self.ticker_fut = None;
                        return Poll::Ready(None);
                    },
                    Poll::Pending => {
                        // The future is still pending, so we can return Pending
                        return Poll::Pending;
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::poll_fn;

    use tari_common_types::types::FixedHash;
    use tari_ootle_common_types::Epoch;

    use super::*;

    #[tokio::test]
    async fn it_picks_up_ticks() {
        let (mut ticker, sender) = watch_ticker();
        // Send a new tick
        let data = EpochTickerData {
            epoch: Epoch(1),
            epoch_hash: FixedHash::default(),
            done_for_now: false,
        };
        sender.send(data.clone()).unwrap();
        let res = poll_fn(|cx| ticker.poll_tick(cx)).await;
        assert_eq!(res, Some(data));

        drop(sender);

        // Poll again after the sender is dropped
        let res = poll_fn(|cx| ticker.poll_tick(cx)).await;
        assert_eq!(res, None, "Expected None after sender is dropped");
    }
}
