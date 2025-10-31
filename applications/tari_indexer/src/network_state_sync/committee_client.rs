//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::Display,
    future::Future,
    ops::{Deref, DerefMut},
};

use log::*;
use tari_epoch_manager::{service::EpochManagerHandle, EpochManagerError, EpochManagerReader};
use tari_networking::NetworkingHandle;
use tari_ootle_common_types::{optional::Optional, PeerAddress, ShardGroup, ToPeerId};
use tari_ootle_p2p::TariMessagingSpec;
use tari_validator_node_rpc::{client::RpcMultiPool, rpc_service, ValidatorNodeRpcClientError};

const LOG_TARGET: &str = "tari::indexer::network_state_sync::committee_client";

#[derive(Debug, Clone)]
pub struct ValidatorCommitteeRpcPool {
    shard_group: ShardGroup,
    pool: RpcMultiPool<TariMessagingSpec>,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    past_failed_nodes: Vec<PeerAddress>,
}

impl ValidatorCommitteeRpcPool {
    pub fn new(
        shard_group: ShardGroup,
        networking: NetworkingHandle<TariMessagingSpec>,
        epoch_manager: EpochManagerHandle<PeerAddress>,
    ) -> Self {
        Self {
            shard_group,
            pool: RpcMultiPool::new(networking),
            epoch_manager,
            past_failed_nodes: vec![],
        }
    }

    pub async fn new_session(&mut self) -> Result<ValidatorRpcSession, ValidatorCommitteeClientError> {
        let epoch = self.epoch_manager.get_current_epoch();
        let mut last_error = None::<ValidatorCommitteeClientError>;
        loop {
            let member = self
                .epoch_manager
                .get_random_committee_member(epoch, Some(self.shard_group), self.past_failed_nodes.clone())
                .await
                .optional()?;

            let Some(member) = member else {
                // All validators have been attempted and failed - no real choice but to clear the past failed nodes and
                // try again if this is called again
                let committee_size = self.past_failed_nodes.len();
                self.past_failed_nodes.clear();
                // Clamp max mem usage to 7300 bytes (Multihash size x 100) - this is likely to always be a no-op
                self.past_failed_nodes.shrink_to(100);
                return Err(ValidatorCommitteeClientError::AllValidatorsFailed {
                    committee_size,
                    last_error: last_error.as_ref().map(|e| e.to_string()),
                });
            };
            let result = self.session_for_peer(member.address).await;
            match result {
                Ok(session) => return Ok(session),
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "Failed to create new session for validator '{}': {}", member, err
                    );
                    last_error = Some(err);
                    self.past_failed_nodes.push(member.address);
                },
            }
        }
    }

    async fn session_for_peer(
        &self,
        address: PeerAddress,
    ) -> Result<ValidatorRpcSession, ValidatorCommitteeClientError> {
        let client = self.pool.get_or_connect(&address.to_peer_id()).await?;
        Ok(ValidatorRpcSession {
            peer_address: address,
            client,
        })
    }

    /// Attempts to execute the provided callback with a session for a random committee member.
    /// If the callback succeeds, it returns the response.
    /// If the callback fails, it will try with another random member until all members have been attempted.
    /// If all members fail, it returns an error with the last error encountered.
    pub async fn try_with_random_members<F, T, E, TFut>(
        &mut self,
        callback: F,
    ) -> Result<T, ValidatorCommitteeClientError>
    where
        F: Fn(ValidatorRpcSession) -> TFut,
        TFut: Future<Output = Result<T, E>>,
        T: 'static,
        E: Display + 'static,
    {
        let epoch = self.epoch_manager.get_current_epoch();
        let mut last_error = None;
        let mut attempted = vec![];
        loop {
            let Some(vn) = self
                .epoch_manager
                .get_random_committee_member(epoch, Some(self.shard_group), attempted.clone())
                .await
                .optional()?
            else {
                warn!(
                    target: LOG_TARGET,
                    "All {} committee members have been attempted and failed.", attempted.len()
                );
                // No more committee members to try
                break;
            };

            let session = match self.session_for_peer(vn.address).await {
                Ok(session) => session,
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "Failed to create session for validator '{}': {}", vn, err
                    );
                    last_error = Some(err.to_string());
                    attempted.push(vn.address);
                    self.past_failed_nodes.push(vn.address);
                    continue; // Skip this member and try the next one
                },
            };

            match callback(session).await {
                Ok(ret) => {
                    return Ok(ret);
                },
                Err(err) => {
                    warn!(
                        target: LOG_TARGET,
                        "Request failed for validator '{}': {}", vn, err
                    );
                    last_error = Some(err.to_string());
                },
            }

            attempted.push(vn.address);
        }

        Err(ValidatorCommitteeClientError::AllValidatorsFailed {
            committee_size: attempted.len(),
            last_error,
        })
    }
}

#[derive(Clone)]
pub struct ValidatorRpcSession {
    peer_address: PeerAddress,
    client: rpc_service::ValidatorNodeRpcClient,
}

impl ValidatorRpcSession {
    pub fn peer_address(&self) -> &PeerAddress {
        &self.peer_address
    }

    pub fn client(&self) -> &rpc_service::ValidatorNodeRpcClient {
        &self.client
    }

    pub fn client_mut(&mut self) -> &mut rpc_service::ValidatorNodeRpcClient {
        &mut self.client
    }
}

impl Deref for ValidatorRpcSession {
    type Target = rpc_service::ValidatorNodeRpcClient;

    fn deref(&self) -> &Self::Target {
        self.client()
    }
}

impl DerefMut for ValidatorRpcSession {
    fn deref_mut(&mut self) -> &mut rpc_service::ValidatorNodeRpcClient {
        self.client_mut()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ValidatorCommitteeClientError {
    #[error("Epoch manager error: {0}")]
    EpochManagerError(#[from] EpochManagerError),
    #[error("Validator node RPC client error: {0}")]
    ValidatorNodeRpcClientError(#[from] ValidatorNodeRpcClientError),
    #[error("Rpc call failed for all ({committee_size}) validators: {}", .last_error.as_deref().unwrap_or("unknown"))]
    AllValidatorsFailed {
        committee_size: usize,
        last_error: Option<String>,
    },
}
