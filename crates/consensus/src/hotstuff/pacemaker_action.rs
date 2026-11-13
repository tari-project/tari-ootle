//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{Epoch, NodeHeight};

/// A change a handler asks of the pacemaker. The worker applies the actions in order once the handler returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacemakerAction {
    /// Propose if this node is the leader and there is something to propose.
    Beat,
    /// Propose at `height` if this node is the leader, even if the block is empty.
    ForceBeat { height: NodeHeight },
    /// Enter `(epoch, height)` and restart both timers, if it is later than the current view.
    EnterView {
        epoch: Epoch,
        height: NodeHeight,
        high_pc_height: NodeHeight,
    },
    /// Restart the leader timeout, leaving the block timer running.
    ResetLeaderTimeout { high_pc_height: NodeHeight },
    /// Move to height zero of `epoch` and restart both timers.
    SetEpoch { epoch: Epoch },
}

/// The pacemaker actions a handler requested. Handlers record into it and the worker applies it after every handler
/// call, including one that returns an error, because a handler can fail after changing the view (e.g. the end of
/// epoch moves to the next epoch and then reports that this node is not registered for it).
#[derive(Debug, Default)]
pub struct PacemakerActions {
    actions: Vec<PacemakerAction>,
}

impl PacemakerActions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn beat(&mut self) {
        self.actions.push(PacemakerAction::Beat);
    }

    pub fn force_beat(&mut self, height: NodeHeight) {
        self.actions.push(PacemakerAction::ForceBeat { height });
    }

    pub fn enter_view(&mut self, epoch: Epoch, height: NodeHeight, high_pc_height: NodeHeight) {
        self.actions.push(PacemakerAction::EnterView {
            epoch,
            height,
            high_pc_height,
        });
    }

    pub fn reset_leader_timeout(&mut self, high_pc_height: NodeHeight) {
        self.actions
            .push(PacemakerAction::ResetLeaderTimeout { high_pc_height });
    }

    pub fn set_epoch(&mut self, epoch: Epoch) {
        self.actions.push(PacemakerAction::SetEpoch { epoch });
    }

    pub fn drain(&mut self) -> impl Iterator<Item = PacemakerAction> + '_ {
        self.actions.drain(..)
    }
}
