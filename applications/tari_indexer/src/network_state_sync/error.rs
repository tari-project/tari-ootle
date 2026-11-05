//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_epoch_manager::EpochManagerError;
use tari_ootle_common_types::{Epoch, StateVersion, ValidatorSetHash, shard::Shard};
use tari_ootle_p2p::PeerAddress;
use tari_ootle_storage::StorageError;
use tari_rpc_framework::{RpcError, RpcStatus};

use crate::network_state_sync::committee_client::ValidatorCommitteeClientError;

#[derive(Debug, thiserror::Error)]
pub enum NetworkStateSyncError {
    #[error("Epoch manager error: {0}")]
    EpochManagerError(#[from] EpochManagerError),
    #[error("Storage error: {0}")]
    StorageError(#[from] StorageError),
    #[error("Validator committee client error: {0}")]
    ValidatorCommitteeClientError(#[from] ValidatorCommitteeClientError),
    #[error("Invalid checkpoint: {details}")]
    InvalidCheckpoint { details: String },
    #[error("Validator served an invalid committed block proof: {details}")]
    InvalidCommitProof { details: String },
    #[error("Invalid state update: {details}")]
    InvalidStateUpdate { details: String },
    /// Stream peers served history no verified leaf will cover: it disagreed with one, was withheld from one, or ran
    /// past every one for too long. What they staged above the shard's last verified version is discarded.
    #[error("{details}")]
    UnverifiedHistory {
        details: String,
        blamed: Vec<PeerAddress>,
        /// The shard whose history after this verified version, or from its v0 when nothing is verified, mismatched a
        /// verified leaf or could not be accumulated, which counts towards halting the shard group once every
        /// member's has.
        diverged_from: Option<(Shard, Option<StateVersion>)>,
    },
    #[error("Request failed: {0}")]
    RequestFailed(#[from] RpcStatus),
    #[error("Rpc error: {0}")]
    RpcError(#[from] RpcError),
    #[error("Invariant error: {details}. This indicates a bug in the code, please report it.")]
    InvariantError { details: String },
    #[error(
        "The network committed validator set {committed} for {epoch}, but this indexer assigned {local}. This indexer \
         disagrees with the network on the validators registered for {epoch} or on how they form committees; check \
         its epoch oracle and committee configuration."
    )]
    ValidatorSetDiverged {
        epoch: Epoch,
        local: ValidatorSetHash,
        committed: ValidatorSetHash,
    },
}

impl NetworkStateSyncError {
    /// True if this failure blames stream peers for history no verified leaf covers, which the shard group answers by
    /// excluding them.
    pub fn is_unverified_history(&self) -> bool {
        matches!(self, Self::UnverifiedHistory { .. })
    }

    /// True if this failure is attributable to the peer or the stream rather than to local state, so
    /// the shards it covers can be left for another round while the rest of the sync proceeds.
    pub fn is_peer_fault(&self) -> bool {
        matches!(
            self,
            Self::RequestFailed(_) |
                Self::RpcError(_) |
                Self::InvalidStateUpdate { .. } |
                Self::UnverifiedHistory { .. } |
                Self::InvalidCheckpoint { .. } |
                Self::InvalidCommitProof { .. } |
                Self::ValidatorCommitteeClientError(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_failure_is_not_attributed_to_the_peer() {
        // Aborting the round is the right response to local state being broken, so these must never be
        // skipped past.
        assert!(
            !NetworkStateSyncError::StorageError(StorageError::NotFound {
                item: "substate",
                key: String::new(),
            })
            .is_peer_fault()
        );
        assert!(!NetworkStateSyncError::InvariantError { details: String::new() }.is_peer_fault());
        assert!(
            !NetworkStateSyncError::ValidatorSetDiverged {
                epoch: Epoch(1),
                local: ValidatorSetHash::from_array([1; 32]),
                committed: ValidatorSetHash::from_array([2; 32]),
            }
            .is_peer_fault()
        );
    }

    #[test]
    fn a_rejection_by_the_peer_is_attributed_to_the_peer() {
        assert!(NetworkStateSyncError::RequestFailed(RpcStatus::bad_request("nope")).is_peer_fault());
        assert!(NetworkStateSyncError::InvalidStateUpdate { details: String::new() }.is_peer_fault());
        assert!(NetworkStateSyncError::InvalidCheckpoint { details: String::new() }.is_peer_fault());
    }
}
