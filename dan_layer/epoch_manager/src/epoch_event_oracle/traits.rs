//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

use crate::epoch_event_oracle::EpochEvent;

pub trait EpochEventOracle {
    /// Returns a Future that returns the next event, completing a round of scanning if necessary.
    /// The implementation must ensure that the returned Future is cancel-safe. Returns None if no further events can be
    /// returned.
    fn next_epoch_event(&mut self) -> impl Future<Output = Option<EpochEvent>> + Send;
}
