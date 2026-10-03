//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub max_concurrent_streams_per_peer: usize,
    /// Upper bound on how long a single message may take to be received or sent once it is under way.
    pub send_recv_timeout: Duration,
    /// How long an inbound stream may sit between messages before it is closed. A sender whose stream is closed
    /// opens a new one for its next message.
    pub inbound_idle_timeout: Duration,
    /// How long an outbound stream may sit with nothing to send before it is closed. This must be shorter than the
    /// peer's `inbound_idle_timeout`, so that the sender ends an idle stream after its last write rather than the
    /// receiver ending it while a message is in flight.
    pub outbound_idle_timeout: Duration,
    pub inbound_message_buffer_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_concurrent_streams_per_peer: 3,
            send_recv_timeout: Duration::from_secs(10),
            inbound_idle_timeout: Duration::from_secs(60),
            outbound_idle_timeout: Duration::from_secs(45),
            inbound_message_buffer_size: 10,
        }
    }
}
