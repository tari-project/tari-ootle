// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_crypto::tari_utilities::ByteArray;
use tari_dan_p2p::proto::rpc::{SyncTemplatesResponse, TemplateType};
use tari_engine_types::TemplateAddress;
use tari_rpc_framework::RpcStatus;
use tari_template_manager::interface::{
    TemplateExecutable,
    TemplateManagerError,
    TemplateManagerHandle,
    TemplateQueryResult,
};
use tokio::{
    sync::{
        broadcast::error::{RecvError, SendError},
        mpsc,
        mpsc::Sender,
    },
    task::JoinError,
};

const LOG_TARGET: &str = "tari::validator_node::template_sync_task";

#[derive(Debug, thiserror::Error)]
pub enum TemplateSyncTaskError {
    #[error("Template address send error: {0}")]
    TemplateAddressSend(#[from] SendError<TemplateAddress>),
    #[error("Template sync response send error: {0}")]
    SyncResponseSend(#[from] mpsc::error::SendError<Result<SyncTemplatesResponse, RpcStatus>>),
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
    tx_responses: Sender<Result<SyncTemplatesResponse, RpcStatus>>,
    template_manager: TemplateManagerHandle,
}

impl TemplateSyncTask {
    pub fn new(
        batch_size: usize,
        template_addresses: Vec<TemplateAddress>,
        tx_responses: Sender<Result<SyncTemplatesResponse, RpcStatus>>,
        template_manager: TemplateManagerHandle,
    ) -> Self {
        Self {
            batch_size,
            template_addresses,
            tx_responses,
            template_manager,
        }
    }

    pub async fn run(mut self) -> Result<(), TemplateSyncTaskError> {
        let num_requested = self.template_addresses.len();
        let mut num_batched = 0;
        let mut num_sent = 0;
        while !self.template_addresses.is_empty() {
            let size = self.batch_size.min(self.template_addresses.len());
            let batch = self.template_addresses.drain(0..size).collect::<Vec<_>>();
            num_batched += batch.len();
            debug!(target: LOG_TARGET, "♻️ fetching {}/{} templates", num_batched, num_requested);
            let templates = self.template_manager.get_templates_by_addresses(batch).await?;
            num_sent += templates.len();
            debug!(target: LOG_TARGET, "♻️ sending {}/{} templates", num_sent, num_requested);
            self.convert_and_send(templates).await?;
        }
        debug!(target: LOG_TARGET, "♻️ sync {}/{} templates complete", num_sent, num_requested);

        Ok(())
    }

    async fn convert_and_send(&self, templates: Vec<TemplateQueryResult>) -> Result<(), TemplateSyncTaskError> {
        for template in templates {
            match template {
                TemplateQueryResult::Found { template } => {
                    let response = match template.executable {
                        TemplateExecutable::CompiledWasm(binary) => Ok(SyncTemplatesResponse {
                            address: template.metadata.address.to_vec(),
                            author_public_key: template.metadata.author_public_key.to_vec(),
                            binary,
                            template_type: TemplateType::Wasm.into(),
                            template_name: template.metadata.name,
                        }),
                        TemplateExecutable::Manifest(manifest) => Ok(SyncTemplatesResponse {
                            address: template.metadata.address.to_vec(),
                            author_public_key: template.metadata.author_public_key.to_vec(),
                            binary: manifest.into_bytes(),
                            template_type: TemplateType::Manifest.into(),
                            template_name: template.metadata.name,
                        }),
                        TemplateExecutable::Flow(flow) => Ok(SyncTemplatesResponse {
                            address: template.metadata.address.to_vec(),
                            author_public_key: template.metadata.author_public_key.to_vec(),
                            binary: flow.into_bytes(),
                            template_type: TemplateType::Flow.into(),
                            template_name: template.metadata.name,
                        }),
                        // this case won't happen as there is no DB type for downloadable WASM
                        TemplateExecutable::DownloadableWasm(_, _) => {
                            Err(RpcStatus::not_implemented("Unsupported type!"))
                        },
                    };

                    self.tx_responses.send(response).await?;
                },
                TemplateQueryResult::NotAvailable { address, status } => {
                    debug!(target: LOG_TARGET, "Template {address} for sync unavailable (status = {status})");
                    // The client can work out for themselves that we didn't send the template
                    continue;
                },
                TemplateQueryResult::NotFound { address } => {
                    debug!(target: LOG_TARGET, "Template {address} for not found");
                    continue;
                },
            };
        }

        Ok(())
    }
}
