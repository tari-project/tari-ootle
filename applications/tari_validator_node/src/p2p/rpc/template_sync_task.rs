// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_dan_app_utilities::template_manager::interface::{Template, TemplateExecutable, TemplateManagerError, TemplateManagerHandle};
use tari_dan_p2p::proto::rpc::{SyncTemplateResponse, TemplateType};
use tari_engine_types::TemplateAddress;
use tari_rpc_framework::RpcStatus;
use tokio::sync::broadcast::error::{RecvError, SendError};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinError;

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
    #[error("Template manager error: {0}")]
    TemplateManager(#[from] TemplateManagerError),
}

/// Async task to collect all the passed templates (by addresses)
/// and send back on [`TemplateSyncTask::tx_responses`] channel.
pub struct TemplateSyncTask {
    batch_size: usize,
    template_addresses: Vec<TemplateAddress>,
    tx_responses: Sender<Result<SyncTemplateResponse, RpcStatus>>,
    template_manager: TemplateManagerHandle,
}

impl TemplateSyncTask {
    pub fn new(
        batch_size: usize,
        template_addresses: Vec<TemplateAddress>,
        tx_responses: Sender<Result<SyncTemplateResponse, RpcStatus>>,
        template_manager: TemplateManagerHandle,
    ) -> Self {
        Self {
            batch_size,
            template_addresses,
            tx_responses,
            template_manager,
        }
    }

    pub async fn run(&self) -> Result<(), TemplateSyncTaskError> {
        for batch in self.template_addresses.chunks(self.batch_size).into_iter() {
            self.convert_and_send(
                self.template_manager
                    .get_templates_by_addresses(batch.into_vec())
                    .await
                    .map_err(|error| TemplateSyncTaskError::TemplateManager(error))?
            )?;
        }

        Ok(())
    }

    async fn convert_and_send(
        &self,
        templates: Vec<Template>,
    ) -> Result<(), TemplateSyncTaskError> {
        for template in templates {
            let result = match template.executable {
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
            };
            self.tx_responses.send(result)?;
        }

        Ok(())
    }
}



