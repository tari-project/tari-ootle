//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display};

use tari_ootle_common_types::Epoch;

use crate::hotstuff::HotStuffError;

#[derive(Debug)]
pub enum ConsensusStateEvent {
    RegisteredForEpoch {
        epoch: Epoch,
    },
    NotRegisteredForEpoch {
        epoch: Epoch,
    },
    /// We are behind peers and need to state-sync. `target_epoch` is the epoch the caller proved
    /// to be the highest finalised one (e.g. via stall-recovery probe). `None` means fall back to
    /// the oracle's current epoch.
    NeedSync {
        target_epoch: Option<Epoch>,
    },
    SyncComplete,
    Ready,
    Failure {
        error: HotStuffError,
    },
    Resume,
    Shutdown,
}

impl Display for ConsensusStateEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[allow(clippy::enum_glob_use)]
        use ConsensusStateEvent::*;
        match self {
            RegisteredForEpoch { epoch } => write!(f, "Registered for epoch {}", epoch),
            NotRegisteredForEpoch { epoch } => write!(f, "Not registered for epoch {}", epoch),
            NeedSync {
                target_epoch: Some(epoch),
            } => write!(f, "Behind peers (target epoch {})", epoch),
            NeedSync { target_epoch: None } => write!(f, "Behind peers"),
            SyncComplete => write!(f, "Sync complete"),
            Ready => write!(f, "Ready"),
            Failure { error } => write!(f, "Failure({error})"),
            Resume => write!(f, "Resume"),
            Shutdown => write!(f, "Shutdown"),
        }
    }
}
