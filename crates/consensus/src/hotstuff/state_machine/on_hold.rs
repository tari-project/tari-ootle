//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::marker::PhantomData;

use log::*;

use crate::{
    hotstuff::{
        HotStuffError,
        state_machine::{event::ConsensusStateEvent, running::Running, worker::ConsensusWorkerContext},
    },
    traits::ConsensusSpec,
};

const LOG_TARGET: &str = "tari::ootle::consensus::sm::on_hold";

/// OnHold is a paused state entered by external request (e.g. an emergency rollback).
/// It blocks until the on-hold watch flag is flipped back to `false`, then returns
/// `OnHoldReleased` so the state machine can transition to `CheckSync`.
///
/// While on hold, the hotstuff worker has already exited its run loop cleanly. Incoming
/// p2p messages are drained and discarded to prevent buffer growth.
#[derive(Debug)]
pub struct OnHold<TSpec> {
    _spec: PhantomData<TSpec>,
}

impl<TSpec> OnHold<TSpec>
where TSpec: ConsensusSpec
{
    pub fn new() -> Self {
        Self { _spec: PhantomData }
    }

    pub(super) async fn on_enter(
        &self,
        context: &mut ConsensusWorkerContext<TSpec>,
    ) -> Result<ConsensusStateEvent, HotStuffError> {
        info!(target: LOG_TARGET, "⏸️ Consensus entering on-hold state");

        let mut rx_on_hold = context.rx_on_hold.clone();
        loop {
            tokio::select! {
                biased;

                changed = rx_on_hold.changed() => {
                    if changed.is_err() {
                        warn!(target: LOG_TARGET, "on-hold watch sender dropped while on-hold — shutting down");
                        return Ok(ConsensusStateEvent::Shutdown);
                    }
                    if !*rx_on_hold.borrow() {
                        info!(target: LOG_TARGET, "▶️ On-hold released, resuming consensus");
                        return Ok(ConsensusStateEvent::OnHoldReleased);
                    }
                    // Still held — continue draining.
                },
                _ = context.hotstuff.discard_messages() => {
                    // discard_messages only returns on shutdown.
                    return Ok(ConsensusStateEvent::Shutdown);
                },
            }
        }
    }
}

impl<TSpec: ConsensusSpec> From<Running<TSpec>> for OnHold<TSpec> {
    fn from(_: Running<TSpec>) -> Self {
        Self::new()
    }
}
