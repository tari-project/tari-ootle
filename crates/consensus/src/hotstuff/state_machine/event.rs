//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display};

use tari_ootle_common_types::{Epoch, ShardGroup};

use crate::hotstuff::HotStuffError;

#[derive(Debug)]
pub enum ConsensusStateEvent {
    /// The base layer has been scanned far enough for this node's registration and committee to be
    /// resolvable.
    Initialised,
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
    /// This node joins `shard_group`'s committee at a later epoch and has not synced its state in the current epoch.
    PendingActivation {
        shard_group: ShardGroup,
    },
    /// The state of the shard group this node joins later was synced while `epoch` was current.
    PresyncComplete {
        epoch: Epoch,
    },
    /// No peer held the checkpoint the presync syncs to yet. `Idle` retries it.
    PresyncDeferred,
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
            Initialised => write!(f, "Initialised"),
            RegisteredForEpoch { epoch } => write!(f, "Registered for epoch {}", epoch),
            NotRegisteredForEpoch { epoch } => write!(f, "Not registered for epoch {}", epoch),
            NeedSync {
                target_epoch: Some(epoch),
            } => write!(f, "Behind peers (target epoch {})", epoch),
            NeedSync { target_epoch: None } => write!(f, "Behind peers"),
            SyncComplete => write!(f, "Sync complete"),
            PendingActivation { shard_group } => write!(f, "Pending activation into {}", shard_group),
            PresyncComplete { epoch } => write!(f, "Presync complete at epoch {}", epoch),
            PresyncDeferred => write!(f, "Presync deferred until its checkpoint is available"),
            Ready => write!(f, "Ready"),
            Failure { error } => write!(f, "Failure({error})"),
            Resume => write!(f, "Resume"),
            Shutdown => write!(f, "Shutdown"),
        }
    }
}
