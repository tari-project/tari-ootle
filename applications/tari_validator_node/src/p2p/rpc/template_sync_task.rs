// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_dan_app_utilities::template_manager::interface::{TemplateExecutable, TemplateManagerError, TemplateManagerHandle};
use tari_dan_p2p::proto::rpc::{SyncTemplateResponse, TemplateType};
use tari_engine_types::TemplateAddress;
use tari_rpc_framework::RpcStatus;
use tokio::select;
use tokio::sync::broadcast::error::{RecvError, SendError};
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::{broadcast, mpsc};
use tokio::task::{JoinError, JoinSet};

#[derive(Debug, thiserror::Error)]
pub enum TemplateSyncTaskError {
    #[error("Template address send error: {0}")]
    TemplateAddressSend(#[from] SendError<TemplateAddress>),
    #[error("Template sync response send error: {0}")]
    SyncResponseSend(#[from] mpsc::error::SendError<Result<SyncTemplateResponse, RpcStatus>>),
    #[error("Task join error: {0}")]
    TaskJoin(#[from] JoinError),
    #[error("Receive error: {0}")]
    Receive(#[from] RecvError),
}

/// Async task to collect all the passed templates (by addresses)
/// and send back on [`TemplateSyncTask::tx_responses`] channel.
///
/// Please note that this task spawns workers to collect all the templates parallel from TemplateManager,
/// so more workers set in [`TemplateSyncTask::new`] speeds up the process but stresses
/// more the TemplateManager.
pub struct TemplateSyncTask {
    workers: usize,
    template_addresses: Vec<TemplateAddress>,
    tx_responses: Sender<Result<SyncTemplateResponse, RpcStatus>>,
    template_manager: TemplateManagerHandle,
}

impl TemplateSyncTask {
    pub fn new(
        workers: usize,
        template_addresses: Vec<TemplateAddress>,
        tx_responses: Sender<Result<SyncTemplateResponse, RpcStatus>>,
        template_manager: TemplateManagerHandle,
    ) -> Self {
        Self {
            workers,
            template_addresses,
            tx_responses,
            template_manager,
        }
    }

    pub async fn run(&self) -> Result<(), TemplateSyncTaskError> {
        let (address_tx, mut address_rx) = broadcast::channel::<TemplateAddress>(self.template_addresses.len());
        let (result_tx, mut result_rx) = mpsc::unbounded_channel::<Result<SyncTemplateResponse, RpcStatus>>();

        // spawning workers
        let mut join_set = JoinSet::new();
        for _ in 0..self.workers {
            join_set.spawn(Self::worker_task(self.template_manager.clone(), address_rx.resubscribe(), result_tx.clone()));
        }

        // sending all addresses to workers
        for address in self.template_addresses {
            address_tx.send(address).await?;
        }

        // waiting for results
        loop {
            select! {
                None = result_rx.recv() => {
                    break;
                }
                Some(result) = result_rx.recv() => {
                    self.tx_responses.send(result).await?;
                }
                Some(Err(error)) = join_set.join_next() => {
                    return Err(TemplateSyncTaskError::TaskJoin(error));
                }
            }
        }

        Ok(())
    }

    async fn worker_task(
        template_manager: TemplateManagerHandle,
        mut address_rx: Receiver<TemplateAddress>,
        result_tx: mpsc::UnboundedSender<Result<SyncTemplateResponse, RpcStatus>>,
    ) -> Result<(), TemplateSyncTaskError> {
        while let address = address_rx.recv().await? {
            let result = match template_manager.get_template(address).await {
                Ok(template) => match template.executable {
                    TemplateExecutable::CompiledWasm(binary) => Ok(SyncTemplateResponse {
                        author_public_key: template.metadata.author_public_key.to_vec(),
                        binary,
                        template_type: TemplateType::Wasm.into(),
                        template_name: template.metadata.name,
                    }),
                    TemplateExecutable::Manifest(manifest) => Ok(SyncTemplateResponse {
                        author_public_key: template.metadata.author_public_key.to_vec(),
                        binary: manifest.into_bytes(),
                        template_type: TemplateType::Manifest.into(),
                        template_name: template.metadata.name,
                    }),
                    TemplateExecutable::Flow(flow) => Ok(SyncTemplateResponse {
                        author_public_key: template.metadata.author_public_key.to_vec(),
                        binary: flow.into_bytes(),
                        template_type: TemplateType::Flow.into(),
                        template_name: template.metadata.name,
                    }),
                    // this case won't happen as there is no DB type for downloadable WASM
                    TemplateExecutable::DownloadableWasm(_, _) => Err(RpcStatus::not_implemented("Unsupported type!")),
                },
                Err(error) => match error {
                    TemplateManagerError::TemplateNotFound { address } => Err(RpcStatus::not_found(
                        format!("Template not found: {}", address).as_str(),
                    )),
                    TemplateManagerError::TemplateUnavailable => Err(RpcStatus::not_found(
                        format!("Template unavailable: {}", address.to_string()).as_str(),
                    )),
                    _ => Err(RpcStatus::general(format!("General error: {}", error).as_str())),
                },
            };
            result_tx.send(result)?;
        }

        Ok(())
    }
}



