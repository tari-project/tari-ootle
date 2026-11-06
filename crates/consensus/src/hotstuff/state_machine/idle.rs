//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::marker::PhantomData;

use log::*;
use tari_epoch_manager::{EpochManagerEvent, EpochManagerReader};
use tari_ootle_common_types::Epoch;
use tokio::sync::broadcast;

use crate::{
    hotstuff::{
        HotStuffError,
        state_machine::{event::ConsensusStateEvent, running::Running, worker::ConsensusWorkerContext},
    },
    traits::ConsensusSpec,
};

const LOG_TARGET: &str = "tari::ootle::consensus::sm::idle";

/// The state of a node that is registered for no committee in the current epoch: it holds whatever it
/// committed while it was, and waits for an epoch that puts it back in one. While a registration that activates
/// later is pending, it presyncs that committee's state once per epoch.
#[derive(Debug, Clone)]
pub struct Idle<TSpec> {
    /// The epoch that was current when the last presync started. A presync syncs to the checkpoint before that
    /// epoch, so repeating it within the same epoch fetches nothing new.
    presynced_at: Option<Epoch>,
    _spec: PhantomData<TSpec>,
}

impl<TSpec> Idle<TSpec>
where TSpec: ConsensusSpec
{
    pub fn new() -> Self {
        Self {
            presynced_at: None,
            _spec: PhantomData,
        }
    }

    pub fn presynced_at(epoch: Epoch) -> Self {
        Self {
            presynced_at: Some(epoch),
            _spec: PhantomData,
        }
    }

    pub(super) async fn on_enter(
        &self,
        context: &mut ConsensusWorkerContext<TSpec>,
    ) -> Result<ConsensusStateEvent, HotStuffError> {
        debug!(target: LOG_TARGET, "Entered idle state");
        // Subscribe before checking if we're registered to eliminate the chance that we miss the epoch event
        let mut epoch_events = context.epoch_manager.subscribe();
        if let Some(event) = self.check_registration(context).await? {
            return Ok(event);
        }
        if let Some(event) = self.check_pending_activation(context).await? {
            return Ok(event);
        }

        loop {
            tokio::select! {
                biased;

                event = epoch_events.recv() => {
                    match event {
                        Ok(event) => {
                            if let Some(event) = self.on_epoch_event(context, event).await? {
                                return Ok(event);
                            }
                        },
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            debug!(target: LOG_TARGET, "Idle state lagged behind by {n} epoch manager events");
                            // The skipped events may include the epoch change that puts this node in a
                            // committee. Only that event promotes out of this state, and the check on
                            // entry has already run, so registration is read again rather than waited
                            // for.
                            if let Some(event) = self.check_registration(context).await? {
                                return Ok(event);
                            }
                            if let Some(event) = self.check_pending_activation(context).await? {
                                return Ok(event);
                            }
                        },
                        Err(broadcast::error::RecvError::Closed) => {
                            debug!(target: LOG_TARGET, "Epoch manager event stream closed");
                            break;
                        },
                    }
                },
                // Ignore hotstuff messages while idle
                _ = context.hotstuff.discard_messages() => { },
            }
        }

        debug!(target: LOG_TARGET, "Idle event triggering shutdown because epoch manager event stream closed");
        Ok(ConsensusStateEvent::Shutdown)
    }

    /// Emits `RegisteredForEpoch` if this node is in a committee for the current epoch.
    async fn check_registration(
        &self,
        context: &mut ConsensusWorkerContext<TSpec>,
    ) -> Result<Option<ConsensusStateEvent>, HotStuffError> {
        let current_epoch = context.epoch_manager.current_epoch().await?;
        if self.is_registered_for_epoch(context, current_epoch).await? {
            return Ok(Some(ConsensusStateEvent::RegisteredForEpoch { epoch: current_epoch }));
        }
        Ok(None)
    }

    async fn is_registered_for_epoch(
        &self,
        context: &mut ConsensusWorkerContext<TSpec>,
        epoch: Epoch,
    ) -> Result<bool, HotStuffError> {
        let is_registered = context
            .epoch_manager
            .is_this_validator_registered_for_epoch(epoch)
            .await?;
        Ok(is_registered)
    }

    /// Emits `PendingActivation` if this node joins a committee at a later epoch and has not presynced in the
    /// current one.
    async fn check_pending_activation(
        &self,
        context: &mut ConsensusWorkerContext<TSpec>,
    ) -> Result<Option<ConsensusStateEvent>, HotStuffError> {
        let Some(pending) = context.epoch_manager.get_pending_activation().await? else {
            return Ok(None);
        };
        let current_epoch = context.epoch_manager.current_epoch().await?;
        if self.presynced_at == Some(current_epoch) {
            return Ok(None);
        }
        info!(
            target: LOG_TARGET,
            "🛜 This node joins {} at {} (current epoch {current_epoch}). Presyncing its state.",
            pending.shard_group,
            pending.activation_epoch,
        );
        Ok(Some(ConsensusStateEvent::PendingActivation {
            shard_group: pending.shard_group,
        }))
    }

    async fn on_epoch_event(
        &self,
        context: &mut ConsensusWorkerContext<TSpec>,
        event: EpochManagerEvent,
    ) -> Result<Option<ConsensusStateEvent>, HotStuffError> {
        match event {
            EpochManagerEvent::EpochChanged { .. } => {
                if let Some(event) = self.check_registration(context).await? {
                    return Ok(Some(event));
                }
                self.check_pending_activation(context).await
            },
            EpochManagerEvent::PendingActivation { .. } => self.check_pending_activation(context).await,
        }
    }
}

impl<TSpec: ConsensusSpec> From<Running<TSpec>> for Idle<TSpec> {
    fn from(_value: Running<TSpec>) -> Self {
        Idle::new()
    }
}
