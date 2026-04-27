//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

use tari_ootle_common_types::Epoch;

use crate::epoch_event_oracle::EpochEvent;

pub trait EpochEventOracle {
    /// Returns a Future that returns the next event, completing a round of scanning if necessary.
    /// The implementation must ensure that the returned Future is cancel-safe. Returns None if no further events can be
    /// returned.
    fn next_epoch_event(&mut self) -> impl Future<Output = Option<EpochEvent>> + Send;

    /// Returns true when, in the oracle's view, `current_epoch` is close enough to ending that
    /// consensus should speculatively accept an `EndEpoch` proposal even if the oracle has not
    /// yet emitted the corresponding `EpochChanged` event.
    ///
    /// "Close enough" is deliberately oracle-specific: the base-layer oracle measures proximity
    /// in base-layer blocks, a wall-clock oracle could measure in seconds, etc. The default
    /// implementation returns `false` (no leeway), which reduces to the strict behaviour of
    /// only accepting `EndEpoch` once the epoch has actually changed locally.
    fn is_within_epoch_end_spread(&self, _current_epoch: Epoch) -> bool {
        false
    }
}
