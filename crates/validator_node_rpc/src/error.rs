//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{str::FromStr, time::Duration};

use tari_bor::BorError;
use tari_networking::{NetworkingError, PeerId};
use tari_ootle_common_types::{ShardGroup, optional::IsNotFoundError};
use tari_rpc_framework::{RpcError, RpcStatus, RpcStatusCode};

/// Builds the `BadRequest` status a validator returns when asked for a substate outside its local `ShardGroup`.
///
/// Paired with [`ValidatorNodeRpcClientError::reported_shard_group`], which extracts `node_shard_group` from the
/// status on the client side.
pub fn does_not_store_substate_status(node_shard_group: ShardGroup, requested: impl std::fmt::Display) -> RpcStatus {
    RpcStatus::bad_request(format!("This node in {node_shard_group} does not store {requested}"))
}

#[derive(Debug, thiserror::Error)]
pub enum ValidatorNodeRpcClientError {
    #[error("NetworkingError: {0}")]
    NetworkingError(#[from] NetworkingError),
    #[error("RpcError: {0}")]
    RpcError(#[from] RpcError),
    #[error("Remote node returned error: {0}")]
    RpcStatusError(#[from] RpcStatus),
    #[error("Node sent invalid response: {0}")]
    InvalidResponse(anyhow::Error),
    #[error("BorError: {0}")]
    BorError(#[from] BorError),
    #[error("Timed out after {timeout:.0?} connecting to peer {peer_id}")]
    ConnectTimeout { peer_id: PeerId, timeout: Duration },
    #[error("Node did not finish its response within {timeout:.0?}")]
    ResponseTimeout { timeout: Duration },
}

impl ValidatorNodeRpcClientError {
    pub fn status(&self) -> Option<&RpcStatus> {
        match self {
            Self::RpcStatusError(status) => Some(status),
            Self::RpcError(RpcError::RequestFailed(status)) => Some(status),
            _ => None,
        }
    }

    /// If the remote node rejected the request via [`does_not_store_substate_status`] because it serves a different
    /// [`ShardGroup`], returns that [`ShardGroup`].
    pub fn reported_shard_group(&self) -> Option<ShardGroup> {
        let status = self.status()?;
        if status.as_status_code() != RpcStatusCode::BadRequest {
            return None;
        }
        let (_, rest) = status.details().split_once("This node in ShardGroup(")?;
        let (range, _) = rest.split_once(") does not store ")?;
        ShardGroup::from_str(range).ok()
    }
}

impl IsNotFoundError for ValidatorNodeRpcClientError {
    fn is_not_found_error(&self) -> bool {
        match self {
            Self::RpcStatusError(status) => status.is_not_found(),
            Self::RpcError(RpcError::RequestFailed(status)) => status.is_not_found(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn does_not_store_status_round_trips_shard_group() {
        let group = ShardGroup::new(1, 128);
        let status = does_not_store_substate_status(group, "component_01");
        let via_status = ValidatorNodeRpcClientError::from(status.clone());
        assert_eq!(via_status.reported_shard_group(), Some(group));
        let via_rpc = ValidatorNodeRpcClientError::from(RpcError::RequestFailed(status));
        assert_eq!(via_rpc.reported_shard_group(), Some(group));
    }
}
