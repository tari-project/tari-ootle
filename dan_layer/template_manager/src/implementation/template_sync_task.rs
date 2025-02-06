//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{HashMap, HashSet};

use log::*;
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{optional::Optional, Epoch, NodeAddressable, ToPeerId};
use tari_dan_p2p::{
    proto::rpc::{SyncTemplatesRequest, SyncTemplatesResponse, TemplateType},
    TariMessagingSpec,
};
use tari_epoch_manager::{base_layer::EpochManagerHandle, EpochManagerError};
use tari_rpc_framework::{ClientStreaming, RpcStatus};
use tari_template_lib::models::TemplateAddress;
use tari_validator_node_rpc::{
    client::{TariValidatorNodeRpcClient, TariValidatorNodeRpcClientFactory, ValidatorNodeClientFactory},
    rpc_service,
};
use tokio_stream::StreamExt;

use crate::implementation::{
    convert_to_db_template_type,
    sync_worker::{SyncedTemplate, TemplateBatchSyncResult, TemplateSyncRequest},
};

const LOG_TARGET: &str = "tari::dan::template_manager::sync_task";

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

pub struct TemplateSyncClientTask<TAddr> {
    client_factory: TariValidatorNodeRpcClientFactory,
    epoch_manager: EpochManagerHandle<TAddr>,
    rpc_client_cache: HashMap<TAddr, TariValidatorNodeRpcClient<TAddr, TariMessagingSpec>>,
    recently_failed_clients: HashSet<TAddr>,
}

impl<TAddr: NodeAddressable + ToPeerId> TemplateSyncClientTask<TAddr> {
    pub fn new(client_factory: TariValidatorNodeRpcClientFactory, epoch_manager: EpochManagerHandle<TAddr>) -> Self {
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

    // pub async fn run(&mut self) -> Result<Vec<TemplateAddress>, TemplateManagerError> {
    //     let address_batches = self.templates_to_sync.chunks(100);
    //     let current_epoch = self.epoch_manager.current_epoch().await?;
    //     let network_info = self.epoch_manager.get_network_committee_info(current_epoch).await?;
    //     let mut failed_addresses = Vec::new();
    //     for addresses in address_batches {
    //         for address in addresses {
    //             let mut sync_successful = false;
    //             let mut client = match self
    //                 .try_get_client_for_template_address(address, &network_info, current_epoch)
    //                 .await?
    //             {
    //                 Ok(client) => client,
    //                 Err(error) => {
    //                     error!(target: LOG_TARGET, "Failed to get client for template address: {error}");
    //                     failed_addresses.push(*address);
    //                     continue;
    //                 },
    //             };
    //
    //             // syncing current part of batch
    //             if let Err(err) = self.try_sync_templates(&mut client, addresses).await {
    //                 error!(target: LOG_TARGET, "Failed to sync templates: {error}");
    //                 failed_addresses.extend(addresses);
    //                 continue;
    //             }
    //             match client
    //                 .sync_templates(SyncTemplatesRequest {
    //                     addresses: addresses.iter().map(|address| address.to_vec()).collect(),
    //                 })
    //                 .await
    //             {
    //                 Ok(mut stream) => {
    //                     while let Some(result) = stream.next().await {
    //                         match result {
    //                             Ok(resp) => {
    //                                 // code
    //                                 let mut compiled_code = None;
    //                                 let mut flow_json = None;
    //                                 let mut manifest = None;
    //                                 let template_type: DbTemplateType;
    //                                 let bin_hash = FixedHash::from(
    //                                     template_hasher32().chain(resp.binary.as_slice()).result().into_array(),
    //                                 );
    //                                 match resp.template_type() {
    //                                     TemplateType::Wasm => {
    //                                         compiled_code = Some(resp.binary);
    //                                         template_type = DbTemplateType::Wasm;
    //                                     },
    //                                     TemplateType::Manifest => {
    //                                         manifest = Some(String::from_utf8(resp.binary)?);
    //                                         template_type = DbTemplateType::Manifest;
    //                                     },
    //                                     TemplateType::Flow => {
    //                                         flow_json = Some(String::from_utf8(resp.binary)?);
    //                                         template_type = DbTemplateType::Flow;
    //                                     },
    //                                 }
    //
    //                                 // get template address
    //                                 let template_address_result = TemplateAddress::try_from_vec(resp.address);
    //                                 if let Err(error) = template_address_result {
    //                                     error!(target: LOG_TARGET, "Invalid template address: {error:?}");
    //                                     continue;
    //                                 }
    //                                 let template_address = template_address_result.unwrap();
    //
    //                                 if let Err(error) = template_manager.update_template(
    //                                     template_address,
    //                                     DbTemplateUpdate::template(
    //                                         FixedHash::try_from(resp.author_public_key.to_vec())?,
    //                                         Some(bin_hash),
    //                                         resp.template_name,
    //                                         template_type,
    //                                         compiled_code,
    //                                         flow_json,
    //                                         manifest,
    //                                     ),
    //                                 ) {
    //                                     error!(target: LOG_TARGET, "Failed to add new template: {error:?}");
    //                                     continue;
    //                                 }
    //
    //                                 // remove from addresses to be able to send back a list of not
    //                                 // synced templates (if any)
    //                                 for (i, addr) in addresses.iter().enumerate() {
    //                                     if *addr == template_address {
    //                                         addresses.remove(i);
    //                                         break;
    //                                     }
    //                                 }
    //
    //                                 sync_successful = true;
    //                                 info!(target: LOG_TARGET, "✅ Template synced successfully: {}",
    // template_address);                                 break;
    //                             },
    //                             Err(error) => {
    //                                 warn!(target: LOG_TARGET, "Can't get stream of templates from VN({addr}):
    // {error:?}");                             },
    //                         }
    //                     }
    //                 },
    //                 Err(error) => {
    //                     warn!(target: LOG_TARGET, "Can't get stream of templates from VN({addr}): {error:?}");
    //                 },
    //             }
    //
    //             if sync_successful {
    //                 break;
    //             }
    //         }
    //     }
    //     Ok(addresses)
    // }

    async fn try_get_client(
        &mut self,
        current_epoch: Epoch,
    ) -> Result<(TAddr, rpc_service::ValidatorNodeRpcClient), TemplateSyncError> {
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

            let mut client = self.client_factory.create_client(&vn.address);
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
