//  Copyright 2023. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

// TODO: rewrite downloader to get template from other peer(s) OR completely drop this concept and implement somewhere
// else

use std::{string::FromUtf8Error, sync::Arc};

use log::*;
use tari_common_types::types::PublicKey;
use tari_crypto::tari_utilities::{ByteArray, ByteArrayError};
use tari_dan_common_types::{
    services::template_provider::TemplateProvider,
    NodeAddressable,
    PeerAddress,
    SubstateAddress,
    TemplateSyncRequest,
};
use tari_dan_engine::function_definitions::FlowFunctionDefinition;
use tari_dan_p2p::proto::{
    rpc as proto,
    rpc::{SyncTemplateResponse, TemplateType},
};
use tari_dan_storage::global::{DbTemplateType, DbTemplateUpdate, TemplateStatus};
use tari_engine_types::{calculate_template_binary_hash, substate::SubstateId};
use tari_epoch_manager::{base_layer::EpochManagerHandle, EpochManagerError, EpochManagerReader};
use tari_rpc_framework::RpcError;
use tari_shutdown::ShutdownSignal;
use tari_template_lib::{models::TemplateAddress, Hash};
use tari_validator_node_client::types::{ArgDef, FunctionDef, TemplateAbi};
use tari_validator_node_rpc::{
    client::{TariValidatorNodeRpcClientFactory, ValidatorNodeClientFactory},
    rpc_service::ValidatorNodeRpcClient,
    ValidatorNodeRpcClientError,
};
use thiserror::Error;
use tokio::{
    sync::{broadcast, mpsc, mpsc::Receiver, oneshot},
    task::JoinHandle,
};

use super::{
    downloader::{DownloadRequest, DownloadResult},
    TemplateManager,
};
use crate::template_manager::interface::{TemplateExecutable, TemplateManagerError, TemplateManagerRequest};

const LOG_TARGET: &str = "tari::template_manager";

#[derive(Error, Debug)]
pub enum TemplateManagerServiceError {
    #[error("Template manager error: {0}")]
    TemplateManager(#[from] TemplateManagerError),
    #[error("Epoch manager error: {0}")]
    EpochManager(#[from] EpochManagerError),
    #[error("Invalid substate ID: {0}")]
    InvalidSubstateId(&'static str),
    #[error("VN RPC client error: {0}")]
    VnRpcClient(#[from] ValidatorNodeRpcClientError),
    #[error("RPC error: {0}")]
    Rpc(#[from] RpcError),
    #[error("Byte array error: {0}")]
    ByteArray(ByteArrayError),
    #[error("UTF-8 bytes to string conversion error: {0}")]
    BytesUtf8Conversion(#[from] FromUtf8Error),
}

pub struct TemplateManagerService<TAddr> {
    rx_request: Receiver<TemplateManagerRequest>,
    manager: TemplateManager<TAddr>,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    completed_downloads: mpsc::Receiver<DownloadResult>,
    download_queue: mpsc::Sender<DownloadRequest>,
    rx_template_sync: broadcast::Receiver<TemplateSyncRequest>,
    client_factory: Arc<TariValidatorNodeRpcClientFactory>,
}

impl<TAddr: NodeAddressable + 'static> TemplateManagerService<TAddr> {
    pub fn spawn(
        rx_request: Receiver<TemplateManagerRequest>,
        manager: TemplateManager<TAddr>,
        epoch_manager: EpochManagerHandle<PeerAddress>,
        download_queue: mpsc::Sender<DownloadRequest>,
        completed_downloads: mpsc::Receiver<DownloadResult>,
        rx_template_sync: broadcast::Receiver<TemplateSyncRequest>,
        client_factory: Arc<TariValidatorNodeRpcClientFactory>,
        shutdown: ShutdownSignal,
    ) -> JoinHandle<anyhow::Result<()>> {
        tokio::spawn(async move {
            Self {
                rx_request,
                manager,
                epoch_manager,
                download_queue,
                completed_downloads,
                rx_template_sync,
                client_factory,
            }
                .run(shutdown)
                .await?;
            Ok(())
        })
    }

    pub async fn run(mut self, mut shutdown: ShutdownSignal) -> Result<(), TemplateManagerError> {
        self.on_startup().await?;
        loop {
            tokio::select! {
                Some(req) = self.rx_request.recv() => self.handle_request(req).await,
                Some(download) = self.completed_downloads.recv() => {
                    if let Err(err) = self.handle_completed_download(download) {
                        error!(target: LOG_TARGET, "Error handling completed download: {}", err);
                    }
                },
                Ok(req) = self.rx_template_sync.recv() => {
                    if let Err(error) = self.handle_template_sync_request(req).await {
                        error!(target: LOG_TARGET, "Failed to handle template sync request: {}", error);
                    }
                }
                _ = shutdown.wait() => {
                    dbg!("Shutting down epoch manager");
                    break;
                }
            }
        }
        Ok(())
    }

    async fn on_startup(&mut self) -> Result<(), TemplateManagerError> {
        let templates = self.manager.fetch_pending_templates()?;
        for template in templates {
            if template.status == TemplateStatus::Pending {
                let _ignore = self
                    .download_queue
                    .send(DownloadRequest {
                        template_type: template.template_type,
                        address: Hash::from(template.template_address.into_array()),
                        expected_binary_hash: template.expected_hash,
                        url: template.url.unwrap(),
                    })
                    .await;
                info!(
                    target: LOG_TARGET,
                    "⏳️️ Template {} queued for download", template.template_address
                );
            }
        }
        Ok(())
    }

    /// Handles request to check if a template by address (substate ID and version) is present locally or needs syncing.
    /// If needs syncing, just does it.
    async fn handle_template_sync_request(
        &mut self,
        req: TemplateSyncRequest,
    ) -> Result<(), TemplateManagerServiceError> {
        // converting to template address from substate ID
        let template_address =
            req.substate_id()
                .substate_id
                .as_template()
                .ok_or(TemplateManagerServiceError::InvalidSubstateId(
                    "Substate ID is not a template ID!",
                ))?;
        let template_address = TemplateAddress::from(template_address.as_hash());

        // we have the template stored already, so no need to do anything with it
        if self.manager.template_exists(&template_address)? {
            return Ok(());
        }

        // sync
        let mut owner_committee = self
            .epoch_manager
            .get_committee_for_substate(
                self.epoch_manager.current_epoch().await?,
                SubstateAddress::from_substate_id(&req.substate_id().substate_id, req.substate_id().version),
            )
            .await?;
        owner_committee.shuffle();

        // start a task to not block other calls
        // TODO: handle result of spawned task, if failed, send again the same TemplateSyncRequest
        // TODO: to the broadcast channel (self.rx_template_sync's sender part)
        let client_factory = self.client_factory.clone();
        let template_manager = Arc::new(self.manager.clone());
        tokio::spawn(async move {
            for (addr, _) in &owner_committee.members {
                match Self::vn_client(client_factory.clone(), addr).await {
                    Ok(mut client) => {
                        match client
                            .sync_template(proto::SyncTemplateRequest {
                                address: template_address.to_vec(),
                            })
                            .await
                        {
                            Ok(resp) => {
                                // TODO: have a proof that this template was added before truly and validate here
                                if let Err(error) = template_manager.add_template(
                                    PublicKey::from_canonical_bytes(resp.author_public_key.as_slice())
                                        .map_err(|error| TemplateManagerServiceError::ByteArray(error))?, // TODO: handle properly
                                    template_address,
                                    match resp.template_type() {
                                        TemplateType::Wasm => TemplateExecutable::CompiledWasm(resp.binary),
                                        TemplateType::Manifest => TemplateExecutable::Manifest(String::from_utf8(resp.binary)?),
                                        TemplateType::Flow => TemplateExecutable::Flow(String::from_utf8(resp.binary)?),
                                    },
                                    None,
                                    Some(TemplateStatus::Active),
                                ) {
                                    error!(target: LOG_TARGET, "Failed to add new template: {error:?}");
                                    return Err(error);
                                }
                                info!(target: LOG_TARGET, "✅ Template synced successfully: {}", template_address);
                            }
                            Err(error) => {
                                warn!(target: LOG_TARGET, "Failed to sync template from VN at address {:?}: {error:?}", addr);
                            }
                        }
                    }
                    Err(error) => {
                        warn!(target: LOG_TARGET, "Failed to obtain connection to VN at address {:?}: {error:?}", addr);
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }

    async fn vn_client(
        client_factory: Arc<TariValidatorNodeRpcClientFactory>,
        addr: &PeerAddress,
    ) -> Result<ValidatorNodeRpcClient, TemplateManagerServiceError> {
        let mut rpc_client = client_factory.create_client(addr);
        let client = rpc_client.client_connection().await?;
        Ok(client)
    }

    async fn handle_request(&mut self, req: TemplateManagerRequest) {
        #[allow(clippy::enum_glob_use)]
        use TemplateManagerRequest::*;
        match req {
            AddTemplate {
                author_public_key,
                template_address,
                template,
                template_name,
                reply,
            } => {
                handle(
                    reply,
                    self.handle_add_template(author_public_key, template_address, template, template_name)
                        .await,
                );
            }
            GetTemplate { address, reply } => {
                handle(reply, self.manager.fetch_template(&address));
            }
            GetTemplates { limit, reply } => handle(reply, self.manager.fetch_template_metadata(limit)),
            LoadTemplateAbi { address, reply } => handle(reply, self.handle_load_template_abi(address)),
        }
    }

    fn handle_load_template_abi(&mut self, address: TemplateAddress) -> Result<TemplateAbi, TemplateManagerError> {
        let loaded = self
            .manager
            .get_template_module(&address)?
            .ok_or(TemplateManagerError::TemplateNotFound { address })?;
        Ok(TemplateAbi {
            template_name: loaded.template_def().template_name().to_string(),
            functions: loaded
                .template_def()
                .functions()
                .iter()
                .map(|f| FunctionDef {
                    name: f.name.clone(),
                    arguments: f
                        .arguments
                        .iter()
                        .map(|a| ArgDef {
                            name: a.name.to_string(),
                            arg_type: a.arg_type.to_string(),
                        })
                        .collect(),
                    output: f.output.to_string(),
                    is_mut: f.is_mut,
                })
                .collect(),
            version: loaded.template_def().tari_version().to_string(),
        })
    }

    fn handle_completed_download(&mut self, download: DownloadResult) -> Result<(), TemplateManagerError> {
        match download.result {
            Ok(bytes) => {
                info!(
                    target: LOG_TARGET,
                    "✅ Template {} downloaded successfully", download.template_address
                );

                // validation of the downloaded template binary hash
                let actual_binary_hash = calculate_template_binary_hash(&bytes);
                let template_status = if actual_binary_hash.as_slice() == download.expected_binary_hash.as_slice() {
                    info!(
                        target: LOG_TARGET,
                        "✅ Template {} is active", download.template_address,
                    );
                    TemplateStatus::Active
                } else {
                    warn!(
                        target: LOG_TARGET,
                        "⚠️ Template {} hash mismatch", download.template_address
                    );
                    // TODO: For now, let's just accept this so that we can update the binary at the URL without
                    // re-registering
                    TemplateStatus::Active
                    // TemplateStatus::Invalid
                };

                let update = match download.template_type {
                    DbTemplateType::Wasm => DbTemplateUpdate {
                        compiled_code: Some(bytes.to_vec()),
                        status: Some(template_status),
                        ..Default::default()
                    },
                    DbTemplateType::Flow => {
                        // make sure it deserializes correctly
                        let mut status = TemplateStatus::Invalid;
                        match serde_json::from_slice::<FlowFunctionDefinition>(&bytes) {
                            Ok(_) => status = template_status,
                            Err(e) => {
                                warn!(
                                    target: LOG_TARGET,
                                    "⚠️ Template {} is not valid json: {}", download.template_address, e
                                );
                            }
                        };

                        DbTemplateUpdate {
                            flow_json: Some(String::from_utf8(bytes.to_vec())?),
                            status: Some(status),
                            ..Default::default()
                        }
                    }
                    DbTemplateType::Manifest => todo!(),
                };
                self.manager.update_template(download.template_address, update)?;
            }
            Err(err) => {
                warn!(target: LOG_TARGET, "🚨 Failed to download template: {}", err);
                self.manager
                    .update_template(download.template_address, DbTemplateUpdate {
                        status: Some(TemplateStatus::DownloadFailed),
                        ..Default::default()
                    })?;
            }
        }
        Ok(())
    }

    async fn handle_add_template(
        &mut self,
        author_public_key: PublicKey,
        template_address: tari_engine_types::TemplateAddress,
        template: TemplateExecutable,
        template_name: Option<String>,
    ) -> Result<(), TemplateManagerError> {
        let template_status = if matches!(template, TemplateExecutable::DownloadableWasm(_, _)) {
            TemplateStatus::New
        } else {
            TemplateStatus::Active
        };
        self.manager.add_template(
            author_public_key,
            template_address,
            template.clone(),
            template_name,
            Some(template_status),
        )?;

        // TODO: remove when we remove support for base layer template registration
        // update template status and add to download queue if it's a downloadable template
        if let TemplateExecutable::DownloadableWasm(url, expected_binary_hash) = template {
            // We could queue this up much later, at which point we'd update to pending
            self.manager.update_template(template_address, DbTemplateUpdate {
                status: Some(TemplateStatus::Pending),
                ..Default::default()
            })?;

            let _ignore = self
                .download_queue
                .send(DownloadRequest {
                    address: template_address,
                    template_type: DbTemplateType::Wasm,
                    url: url.to_string(),
                    expected_binary_hash,
                })
                .await;
        }

        Ok(())
    }
}

fn handle<T>(reply: oneshot::Sender<Result<T, TemplateManagerError>>, result: Result<T, TemplateManagerError>) {
    if let Err(ref e) = result {
        error!(target: LOG_TARGET, "Request failed with error: {}", e);
    }
    if reply.send(result).is_err() {
        error!(target: LOG_TARGET, "Requester abandoned request");
    }
}
