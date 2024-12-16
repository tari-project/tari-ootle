// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::TemplateSyncRequest;
use tari_dan_storage::consensus_models::TransactionRecord;
use tari_engine_types::substate::SubstateId;
use tokio::sync::{broadcast, broadcast::error::SendError};

#[derive(Debug, thiserror::Error)]
pub enum TemplateSyncError {
    #[error("Failed to send template address to sync: {0}")]
    TemplateSyncSend(#[from] SendError<TemplateSyncRequest>),
}

/// Sends all inputs of the transaction where the input is a template to template manager,
/// so it can do synchronization if needed.
pub async fn sync_templates(
    tx_template_sync: broadcast::Sender<TemplateSyncRequest>,
    transaction: &TransactionRecord,
) -> Result<(), TemplateSyncError> {
    if let Some(inputs) = &transaction.resolved_inputs {
        for versioned_substate_id in inputs {
            if matches!(versioned_substate_id.substate_id(), SubstateId::Template(_)) {
                tx_template_sync.send(TemplateSyncRequest::new(
                    versioned_substate_id.substate_id().clone(),
                    versioned_substate_id.version(),
                ))?;
            }
        }
    }

    Ok(())
}
