//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{HashMap, HashSet};

use log::*;
use tari_common_types::types::FixedHash;
use tari_epoch_manager::{EpochManagerError, EpochManagerReader};
use tari_ootle_common_types::{optional::Optional, Epoch, NodeAddressable, ToPeerId};
use tari_ootle_p2p::{
    proto::rpc::{SyncTemplatesRequest, SyncTemplatesResponse, TemplateType},
    TariMessagingSpec,
};
use tari_rpc_framework::{ClientStreaming, RpcStatus};
use tari_template_lib::types::TemplateAddress;
use tari_validator_node_rpc::{
    client::{TariValidatorNodeRpcClient, TariValidatorNodeRpcClientFactory, ValidatorNodeClientFactory},
    rpc_service,
};
use tokio_stream::StreamExt;

use crate::implementation::{
    convert_to_db_template_type,
    sync_worker::{SyncedTemplate, TemplateBatchSyncResult, TemplateSyncRequest},
};

const LOG_TARGET: &str = "tari::ootle::template_manager::sync_task";

#[derive(Debug, thiserror::Error)]
pub enum TemplateSyncError {
    #[error("Failed to download template: {0}")]
    DownloadFailed(#[from] reqwest::Error),
    #[error("Peer {peer_address} returned an invalid response {error}")]
    InvalidResponse { peer_address: String, error: String },
    #[error(
        "Template {template_address} verification failed: binary hash ({actual_hash}) did not match expected hash \
         ({expected_hash})"
    )]
    TemplateHashMismatch {
        template_address: TemplateAddress,
        expected_hash: FixedHash,
        actual_hash: FixedHash,
    },
    #[error("There are no more sync validators to try")]
    NoMoreSyncValidators,
    #[error("Epoch manager error: {0}")]
    EpochManagerError(#[from] EpochManagerError),
}

pub struct TemplateSyncClientTask<TAddr, TEpochManager> {
    client_factory: TariValidatorNodeRpcClientFactory,
    epoch_manager: TEpochManager,
    rpc_client_cache: HashMap<TAddr, TariValidatorNodeRpcClient<TAddr, TariMessagingSpec>>,
    recently_failed_clients: HashSet<TAddr>,
}

impl<TEpochManager> TemplateSyncClientTask<TEpochManager::Addr, TEpochManager>
where
    TEpochManager: EpochManagerReader,
    TEpochManager::Addr: NodeAddressable + ToPeerId,
{
    pub fn new(client_factory: TariValidatorNodeRpcClientFactory, epoch_manager: TEpochManager) -> Self {
        Self {
            client_factory,
            epoch_manager,
            rpc_client_cache: HashMap::new(),
            recently_failed_clients: HashSet::new(),
        }
    }

    pub async fn run(&mut self, requests: Vec<TemplateSyncRequest>, current_epoch: Epoch) -> TemplateBatchSyncResult {
        let mut sync_result = TemplateBatchSyncResult::default();

        let mut requests = requests
            .into_iter()
            .map(|req| (req.address, req))
            .collect::<HashMap<_, _>>();

        'choose_a_new_peer: loop {
            let (peer_address, mut stream) = loop {
                let (address, mut client) = match self.try_get_client(current_epoch).await {
                    Ok(v) => v,
                    Err(err) => {
                        sync_result.unfulfilled = requests.into_values().collect();
                        sync_result.sync_aborted = Some(err);
                        return sync_result;
                    },
                };

                debug!(target: LOG_TARGET, "Connected to peer {}. Attempting to start sync", address);

                // syncing current part of batch
                match self.try_start_stream(&mut client, requests.keys()).await {
                    Ok(stream) => break (address, stream),
                    Err(err) => {
                        warn!(target: LOG_TARGET, "Failed to start template stream: {err}");
                        self.recently_failed_clients.insert(address);
                        continue;
                    },
                }
            };

            debug!(target: LOG_TARGET, "Starting template sync from peer {peer_address}");
            while let Some(resp) = stream.next().await {
                match resp {
                    Ok(resp) => {
                        let (address, template) = match convert_resp(&peer_address, resp) {
                            Ok(resp) => resp,
                            Err(err) => {
                                warn!(target: LOG_TARGET, "⚠️ Invalid response from peer {peer_address}: {err}");
                                self.recently_failed_clients.insert(peer_address);
                                continue 'choose_a_new_peer;
                            },
                        };
                        let Some(req) = requests.remove(&address) else {
                            warn!(target: LOG_TARGET, "⚠️ Peer {peer_address} sent a template {address} we did not request!");
                            // TODO: select another peer
                            continue;
                        };

                        if let Err(err) = validate(&req, &template) {
                            warn!(target: LOG_TARGET, "⚠️ Invalid template from peer {peer_address}: {err}");
                            sync_result.failed.push((req, err));
                            continue;
                        }

                        sync_result.synced.insert(address, template);
                        debug!(target: LOG_TARGET, "🟢 Template synced successfully: {}", address);
                    },
                    Err(err) => {
                        warn!(target: LOG_TARGET, "⚠️ Template stream failed for peer {peer_address}: {err}");
                        continue 'choose_a_new_peer;
                    },
                }
            }

            // Any remaining requests are not fulfilled
            sync_result.unfulfilled = requests.into_values().collect();
            break;
        }

        sync_result
    }

    async fn try_start_stream<'a, I: IntoIterator<Item = &'a TemplateAddress>>(
        &self,
        client: &mut rpc_service::ValidatorNodeRpcClient,
        requests: I,
    ) -> Result<ClientStreaming<SyncTemplatesResponse>, RpcStatus> {
        let stream = client
            .sync_templates(SyncTemplatesRequest {
                addresses: requests.into_iter().map(|addr| addr.to_vec()).collect(),
            })
            .await?;

        Ok(stream)
    }

    async fn try_get_client(
        &mut self,
        current_epoch: Epoch,
    ) -> Result<(TEpochManager::Addr, rpc_service::ValidatorNodeRpcClient), TemplateSyncError> {
        for (peer_address, client) in &self.rpc_client_cache {
            if self.recently_failed_clients.contains(peer_address) {
                continue;
            }

            match client.clone().client_connection().await {
                Ok(conn) => {
                    self.recently_failed_clients.remove(peer_address);
                    return Ok((peer_address.clone(), conn));
                },
                Err(error) => {
                    error!(target: LOG_TARGET, "Failed to connect to VN: {error}");
                    self.recently_failed_clients.insert(peer_address.clone());
                },
            }
        }

        for peer in &self.recently_failed_clients {
            self.rpc_client_cache.remove(peer);
        }

        let (peer_address, client, conn) = loop {
            let Some(vn) = self
                .epoch_manager
                .get_random_committee_member(
                    current_epoch,
                    None,
                    self.recently_failed_clients.iter().cloned().collect(),
                )
                .await
                .optional()?
            else {
                return Err(TemplateSyncError::NoMoreSyncValidators);
            };

            let client = self.client_factory.create_client(&vn.address);
            match client.client_connection().await {
                Ok(conn) => {
                    self.recently_failed_clients.remove(&vn.address);
                    break (vn.address.clone(), client, conn);
                },
                Err(error) => {
                    self.recently_failed_clients.insert(client.address().clone());
                    error!(target: LOG_TARGET, "Failed to connect to VN: {error}");
                },
            }
        };

        self.rpc_client_cache.insert(peer_address.clone(), client);
        Ok((peer_address, conn))
    }
}

fn convert_resp<TAddr: NodeAddressable>(
    peer_address: &TAddr,
    resp: SyncTemplatesResponse,
) -> Result<(TemplateAddress, SyncedTemplate), TemplateSyncError> {
    let address =
        TemplateAddress::try_from(resp.address.as_slice()).map_err(|e| TemplateSyncError::InvalidResponse {
            peer_address: peer_address.to_string(),
            error: format!("invalid template address: {e}"),
        })?;
    let template_type = TemplateType::try_from(resp.template_type).map_err(|e| TemplateSyncError::InvalidResponse {
        peer_address: peer_address.to_string(),
        error: format!("invalid template type: {e}"),
    })?;
    let template_type = convert_to_db_template_type(template_type);

    Ok((address, SyncedTemplate {
        template_type,
        binary: resp.binary,
    }))
}

fn validate(req: &TemplateSyncRequest, sync_template: &SyncedTemplate) -> Result<(), TemplateSyncError> {
    let hash = sync_template.hash_binary();
    if req.expected_binary_hash != hash {
        return Err(TemplateSyncError::TemplateHashMismatch {
            template_address: req.address,
            expected_hash: req.expected_binary_hash,
            actual_hash: hash,
        });
    }
    Ok(())
}
