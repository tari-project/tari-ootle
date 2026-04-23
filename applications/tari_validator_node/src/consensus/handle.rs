//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_consensus::hotstuff::{ConsensusCurrentState, CurrentView, HotstuffEvent};
use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::Transaction;
use tokio::sync::{broadcast, mpsc, watch};

use crate::event_subscription::EventSubscription;

#[derive(Debug, thiserror::Error)]
pub enum ConsensusHandleError {
    #[error("Consensus worker is shut down")]
    Shutdown,
    #[error("Timed out waiting for consensus state to become {expected}")]
    StateTransitionTimeout { expected: &'static str },
}

#[derive(Debug, Clone)]
pub struct ConsensusHandle {
    rx_current_state: watch::Receiver<ConsensusCurrentState>,
    events_subscription: EventSubscription<HotstuffEvent>,
    current_view: CurrentView,
    tx_new_transaction: mpsc::Sender<(Transaction, usize)>,
    tx_on_hold: watch::Sender<bool>,
}

impl ConsensusHandle {
    pub(super) fn new(
        rx_current_state: watch::Receiver<ConsensusCurrentState>,
        events_subscription: EventSubscription<HotstuffEvent>,
        current_view: CurrentView,
        tx_new_transaction: mpsc::Sender<(Transaction, usize)>,
        tx_on_hold: watch::Sender<bool>,
    ) -> Self {
        Self {
            rx_current_state,
            events_subscription,
            current_view,
            tx_new_transaction,
            tx_on_hold,
        }
    }

    pub fn current_epoch(&self) -> Epoch {
        self.current_view.get_epoch()
    }

    pub async fn notify_new_transaction(
        &self,
        transaction: Transaction,
        num_pending: usize,
    ) -> Result<(), mpsc::error::SendError<()>> {
        self.tx_new_transaction
            .send((transaction, num_pending))
            .await
            .map_err(|_| mpsc::error::SendError(()))
    }

    pub fn current_view(&self) -> &CurrentView {
        &self.current_view
    }

    pub fn subscribe_to_hotstuff_events(&mut self) -> Result<broadcast::Receiver<HotstuffEvent>, anyhow::Error> {
        Ok(self.events_subscription.try_subscribe()?)
    }

    pub fn get_current_state(&self) -> ConsensusCurrentState {
        *self.rx_current_state.borrow()
    }

    pub fn is_running(&self) -> bool {
        self.get_current_state().is_running()
    }

    pub fn is_on_hold(&self) -> bool {
        self.get_current_state().is_on_hold()
    }

    /// Request that the consensus state machine enter `OnHold`. Blocks until the state
    /// machine reports `ConsensusCurrentState::OnHold` (which guarantees the hotstuff
    /// worker has unwound its run loop) or the timeout elapses.
    pub async fn request_on_hold(&self, timeout: Duration) -> Result<(), ConsensusHandleError> {
        self.tx_on_hold.send(true).map_err(|_| ConsensusHandleError::Shutdown)?;
        self.wait_for_state(ConsensusCurrentState::OnHold, timeout, "OnHold")
            .await
    }

    /// Release a pending on-hold. Blocks until the state machine has left `OnHold`.
    pub async fn release_on_hold(&self, timeout: Duration) -> Result<(), ConsensusHandleError> {
        self.tx_on_hold
            .send(false)
            .map_err(|_| ConsensusHandleError::Shutdown)?;
        self.wait_for_state_change_from(ConsensusCurrentState::OnHold, timeout, "not OnHold")
            .await
    }

    async fn wait_for_state(
        &self,
        target: ConsensusCurrentState,
        timeout: Duration,
        label: &'static str,
    ) -> Result<(), ConsensusHandleError> {
        let mut rx = self.rx_current_state.clone();
        if *rx.borrow() == target {
            return Ok(());
        }
        let fut = async {
            while rx.changed().await.is_ok() {
                if *rx.borrow() == target {
                    return Ok(());
                }
            }
            Err(ConsensusHandleError::Shutdown)
        };
        tokio::time::timeout(timeout, fut)
            .await
            .map_err(|_| ConsensusHandleError::StateTransitionTimeout { expected: label })?
    }

    async fn wait_for_state_change_from(
        &self,
        current: ConsensusCurrentState,
        timeout: Duration,
        label: &'static str,
    ) -> Result<(), ConsensusHandleError> {
        let mut rx = self.rx_current_state.clone();
        if *rx.borrow() != current {
            return Ok(());
        }
        let fut = async {
            while rx.changed().await.is_ok() {
                if *rx.borrow() != current {
                    return Ok(());
                }
            }
            Err(ConsensusHandleError::Shutdown)
        };
        tokio::time::timeout(timeout, fut)
            .await
            .map_err(|_| ConsensusHandleError::StateTransitionTimeout { expected: label })?
    }
}
