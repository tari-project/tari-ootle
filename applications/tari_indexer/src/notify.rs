//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct Notify<T> {
    publisher: broadcast::Sender<T>,
}

impl<T: Clone> Notify<T> {
    pub fn new(capacity: usize) -> Self {
        let (publisher, _) = broadcast::channel(capacity);
        Self { publisher }
    }

    /// Creates a "read-only" `Subscriber` that can be used to create new subscriptions but is not able to publish.
    pub fn to_subscriber(&self) -> Subscriber<T> {
        Subscriber {
            inner: self.publisher.clone(),
        }
    }

    pub fn notify<V: Into<T>>(&self, value: V) {
        let _err = self.publisher.send(value.into());
    }
}

#[derive(Debug, Clone)]
pub struct Subscriber<T> {
    inner: broadcast::Sender<T>,
}

impl<T: Clone> Subscriber<T> {
    pub fn subscribe(&self) -> broadcast::Receiver<T> {
        self.inner.subscribe()
    }
}
