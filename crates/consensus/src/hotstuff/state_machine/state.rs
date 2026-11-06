//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::hotstuff::state_machine::{
    check_sync::CheckSync,
    idle::Idle,
    initialising::Initialising,
    running::Running,
    syncing::Syncing,
};

#[derive(Debug)]
pub(super) enum ConsensusState<TSpec> {
    Initialising(Initialising<TSpec>),
    Idle(Idle<TSpec>),
    CheckSync(CheckSync<TSpec>),
    Syncing(Syncing<TSpec>),
    Running(Running<TSpec>),
    Sleeping,
    Shutdown,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConsensusCurrentState {
    #[default]
    Initialising,
    Idle,
    CheckSync,
    Syncing,
    /// Syncing the state of the committee this node joins at a later epoch, outside consensus.
    Presyncing,
    Running,
    Sleeping,
    Shutdown,
}

impl Display for ConsensusCurrentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[allow(clippy::enum_glob_use)]
        use ConsensusCurrentState::*;
        match self {
            Initialising => write!(f, "Initialising"),
            Idle => write!(f, "Idle"),
            CheckSync => write!(f, "CheckSync"),
            Syncing => write!(f, "Syncing"),
            Presyncing => write!(f, "Presyncing"),
            Running => write!(f, "Running"),
            Sleeping => write!(f, "Sleeping"),
            Shutdown => write!(f, "Shutdown"),
        }
    }
}

impl ConsensusCurrentState {
    pub fn is_running(&self) -> bool {
        matches!(self, ConsensusCurrentState::Running)
    }
}

impl<TSpec> ConsensusState<TSpec> {
    pub fn is_shutdown(&self) -> bool {
        matches!(self, ConsensusState::Shutdown)
    }
}

impl<TSpec> Display for ConsensusState<TSpec> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        ConsensusCurrentState::from(self).fmt(f)
    }
}

impl<TSpec> From<&ConsensusState<TSpec>> for ConsensusCurrentState {
    fn from(value: &ConsensusState<TSpec>) -> Self {
        match value {
            ConsensusState::Initialising(_) => ConsensusCurrentState::Initialising,
            ConsensusState::Idle(_) => ConsensusCurrentState::Idle,
            ConsensusState::CheckSync(_) => ConsensusCurrentState::CheckSync,
            ConsensusState::Syncing(state) if state.is_presync() => ConsensusCurrentState::Presyncing,
            ConsensusState::Syncing(_) => ConsensusCurrentState::Syncing,
            ConsensusState::Running(_) => ConsensusCurrentState::Running,
            ConsensusState::Sleeping => ConsensusCurrentState::Sleeping,
            ConsensusState::Shutdown => ConsensusCurrentState::Shutdown,
        }
    }
}
