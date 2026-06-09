//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_epoch_manager::EpochManagerError;
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
    #[error("Request failed: {0}")]
    RequestFailed(#[from] RpcStatus),
    #[error("Rpc error: {0}")]
    RpcError(#[from] RpcError),
    #[error("Invariant error: {details}. This indicates a bug in the code, please report it.")]
    InvariantError { details: String },
}
