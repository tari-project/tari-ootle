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

use log::*;
use tari_engine::{template::LoadedTemplate, wasm::WasmModule};
use tari_engine_types::calculate_template_binary_hash;
use tari_epoch_manager::EpochManagerReader;
use tari_ootle_common_types::{services::template_provider::TemplateProvider, Epoch, NodeAddressable, ToPeerId};
use tari_ootle_p2p::proto::rpc::TemplateType;
use tari_ootle_storage::global::{DbTemplateType, DbTemplateUpdate, TemplateStatus};
use tari_shutdown::ShutdownSignal;
use tari_template_lib::types::{crypto::RistrettoPublicKeyBytes, Hash, TemplateAddress};
use tari_validator_node_rpc::client::TariValidatorNodeRpcClientFactory;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use super::{
    downloader::{DownloadRequest, DownloadResult},
    TemplateManager,
};
use crate::{
    implementation::sync_worker::{SyncWorkerEvent, TemplateSyncRequest, TemplateSyncWorker},
    interface::{
        AddTemplateRequest,
        TemplateChange,
        TemplateExecutable,
        TemplateManagerError,
        TemplateManagerRequest,
        TemplateQueryResult,
    },
};

const LOG_TARGET: &str = "tari::ootle::template_manager";

pub struct TemplateManagerService<TAddr, TEpochManager> {
    rx_request: mpsc::Receiver<TemplateManagerRequest>,
    manager: TemplateManager<TAddr>,
    completed_downloads: mpsc::Receiver<DownloadResult>,
    download_queue: mpsc::Sender<DownloadRequest>,
    download_requests: mpsc::UnboundedReceiver<AddTemplateRequest>,
    sync_worker: TemplateSyncWorker<TEpochManager>,
}

impl<TEpochManager> TemplateManagerService<TEpochManager::Addr, TEpochManager>
where
    TEpochManager: EpochManagerReader + Clone + Send + Sync + 'static,
    TEpochManager::Addr: NodeAddressable + ToPeerId + 'static,
{
    pub fn spawn(
        rx_request: mpsc::Receiver<TemplateManagerRequest>,
        manager: TemplateManager<TEpochManager::Addr>,
        epoch_manager: TEpochManager,
        download_queue: mpsc::Sender<DownloadRequest>,
        download_requests: mpsc::UnboundedReceiver<AddTemplateRequest>,
        completed_downloads: mpsc::Receiver<DownloadResult>,
        client_factory: TariValidatorNodeRpcClientFactory,
        shutdown: ShutdownSignal,
    ) -> JoinHandle<anyhow::Result<()>> {
        tokio::spawn(async move {
            Self {
                rx_request,
                manager,
                download_queue,
                download_requests,
                completed_downloads,
                sync_worker: TemplateSyncWorker::new(epoch_manager, client_factory),
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
                Some(req) = self.download_requests.recv() => {
                     if let Err(err) = self.handle_add_template(req.author_public_key, req.template_address, req.template, req.template_name, req.epoch) .await {
                        error!(target: LOG_TARGET, "Error handling download request: {}", err);
                    }
                },
                Some(download) = self.completed_downloads.recv() => {
                    if let Err(err) = self.handle_completed_download(download) {
                        error!(target: LOG_TARGET, "Error handling completed download: {}", err);
                    }
                },

                event = self.sync_worker.next() => {
                    self.handle_sync_worker_event(event);
                },
                _ = shutdown.wait() => {
                    info!(target: LOG_TARGET, "💤 Shutting down template manager");
                    break;
                }
            }
        }
        Ok(())
    }

    fn handle_sync_worker_event(&mut self, event: SyncWorkerEvent) {
        info!(target: LOG_TARGET, "♻️ Template sync worker event: {}", event);
        match event {
            SyncWorkerEvent::SyncRoundCompleted { result } => {
                for (address, template) in result.synced {
                    let update = match WasmModule::load_template_from_code(template.binary.as_slice()) {
                        Ok(module) => DbTemplateUpdate {
                            template_name: Some(module.template_name().to_string()),
                            template_type: Some(template.template_type),
                            code: Some(template.binary),
                            status: Some(TemplateStatus::Active),
                            ..Default::default()
                        },
                        Err(err) => {
                            // Sounds the alarm bells if this ever happens (probably cause, backward compatibility for
                            // old wasms)
                            error!(target: LOG_TARGET, "❌❗️ Template {} was committed in consensus and verified to have the correct binary hash but could not load: {}", address, err);
                            DbTemplateUpdate {
                                template_name: None,
                                code: Some(template.binary),
                                status: Some(TemplateStatus::Invalid),
                                ..Default::default()
                            }
                        },
                    };

                    self.manager.update_template(address, update).unwrap()
                }

                if let Some(err) = result.sync_aborted {
                    warn!(target: LOG_TARGET, "⚠️ Sync was aborted due to an error: {}", err);
                }

                if !result.unfulfilled.is_empty() {
                    info!(target: LOG_TARGET, "❓️ {} template(s) not synced. These will be requeued", result.unfulfilled.len());
                    self.sync_worker.enqueue_all(result.unfulfilled);
                }

                for (request, err) in &result.failed {
                    warn!(target: LOG_TARGET, "⚠️ Sync errored for template {}: {}. Requeing...", request.address, err);
                }

                self.sync_worker
                    .enqueue_all(result.failed.into_iter().map(|(request, _)| request));
            },
            SyncWorkerEvent::SyncError { error, batch } => {
                error!(target: LOG_TARGET, "Sync worker error {} sync requests aborted: {}.", batch.len(), error);
                self.sync_worker.enqueue_all(batch);
            },
        }
    }

    async fn on_startup(&mut self) -> Result<(), TemplateManagerError> {
        let templates = self.manager.fetch_pending_templates()?;

        let mut batch = vec![];
        // trigger syncing templates the old way too
        for template in templates {
            if template.status != TemplateStatus::Pending {
                continue;
            }
            if template.url.is_some() {
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
            } else {
                info!(
                    target: LOG_TARGET,
                    "⏳️️ Template {} queued for sync", template.template_address
                );
                batch.push(TemplateSyncRequest {
                    address: template.template_address,
                    expected_binary_hash: template.expected_hash,
                });
            }
        }
        self.sync_worker.enqueue_all(batch);
        Ok(())
    }

    async fn handle_request(&mut self, req: TemplateManagerRequest) {
        #[allow(clippy::enum_glob_use)]
        use TemplateManagerRequest::*;
        match req {
            AddTemplate {
                request:
                    AddTemplateRequest {
                        author_public_key,
                        template_address,
                        template,
                        template_name,
                        epoch,
                    },
                reply,
            } => {
                handle(
                    reply,
                    self.handle_add_template(author_public_key, template_address, template, template_name, epoch)
                        .await,
                );
            },
            GetTemplate { address, reply } => {
                handle(reply, self.manager.fetch_template(&address));
            },
            GetTemplates { limit, reply } => handle(reply, self.manager.fetch_template_metadata(limit)),
            LoadTemplateAbi { address, reply } => handle(reply, self.handle_load_template_abi(address)),
            TemplateExists { address, status, reply } => handle(reply, self.handle_template_exists(&address, status)),
            GetTemplatesByAddresses { addresses, reply } => handle(
                reply,
                self.handle_get_templates_by_addresses(addresses.into_iter().collect()),
            ),
            EnqueueTemplateChanges {
                template_changes,
                reply,
            } => handle(
                reply,
                self.handle_enqueue_template_changes_request(template_changes).await,
            ),
        }
    }

    fn handle_load_template_abi(&mut self, address: TemplateAddress) -> Result<LoadedTemplate, TemplateManagerError> {
        let loaded = self
            .manager
            .get_template_module(&address)?
            .ok_or(TemplateManagerError::TemplateNotFound { address })?;
        Ok(loaded)
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
                        code: Some(bytes.to_vec()),
                        status: Some(template_status),
                        ..Default::default()
                    },
                    DbTemplateType::Manifest => todo!(),
                };
                self.manager.update_template(download.template_address, update)?;
            },
            Err(err) => {
                warn!(target: LOG_TARGET, "🚨 Failed to download template: {}", err);
                self.manager
                    .update_template(download.template_address, DbTemplateUpdate {
                        status: Some(TemplateStatus::DownloadFailed),
                        ..Default::default()
                    })?;
            },
        }
        Ok(())
    }

    /// Handling template exists request.
    fn handle_template_exists(
        &mut self,
        template_address: &TemplateAddress,
        status: Option<TemplateStatus>,
    ) -> Result<bool, TemplateManagerError> {
        self.manager.template_exists(template_address, status)
    }

    /// Handling fetching templates by addresses.
    fn handle_get_templates_by_addresses(
        &mut self,
        addresses: Vec<TemplateAddress>,
    ) -> Result<Vec<TemplateQueryResult>, TemplateManagerError> {
        self.manager.fetch_templates_by_addresses(addresses)
    }

    async fn handle_add_template(
        &mut self,
        author_public_key: RistrettoPublicKeyBytes,
        template_address: TemplateAddress,
        template: TemplateExecutable,
        template_name: Option<String>,
        epoch: Epoch,
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
            epoch,
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

    async fn handle_enqueue_template_changes_request(
        &mut self,
        changes: Vec<TemplateChange>,
    ) -> Result<(), TemplateManagerError> {
        let mut sync_requests = vec![];
        for change in changes {
            match change {
                TemplateChange::Add {
                    template_address,
                    author_public_key,
                    binary_hash,
                    epoch,
                } => {
                    if self
                        .manager
                        .template_exists(&template_address.as_hash(), Some(TemplateStatus::Active))?
                    {
                        info!(target: LOG_TARGET, "❓️ Template {} already exists and is active. Not enqueuing for sync", template_address);
                        continue;
                    }
                    if self
                        .manager
                        .template_exists(&template_address.as_hash(), Some(TemplateStatus::Deprecated))?
                    {
                        info!(target: LOG_TARGET, "❓️ Template {} already exists and is active(deprecated). Not enqueuing for sync", template_address);
                        continue;
                    }

                    self.manager.add_pending_template(
                        // Once we download the template we'll extract the name. This saves us from having to store the
                        // name in the substate
                        "<unknown>".to_string(),
                        template_address.as_hash(),
                        author_public_key,
                        binary_hash,
                        epoch,
                        TemplateType::Wasm,
                    )?;

                    sync_requests.push(TemplateSyncRequest {
                        address: template_address.as_hash(),
                        expected_binary_hash: binary_hash,
                    });
                },
                TemplateChange::Deprecate { template_address } => {
                    self.manager.deprecate_template(&template_address.as_hash())?;
                },
            }
        }
        self.sync_worker.enqueue_all(sync_requests);
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
