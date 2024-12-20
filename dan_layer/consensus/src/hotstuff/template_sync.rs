// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::info;
use tari_dan_common_types::TemplateSyncRequest;
use tari_dan_storage::consensus_models::TransactionRecord;
use tari_engine_types::{commit_result::TransactionResult, instruction::Instruction, substate::SubstateId};
use tokio::sync::{broadcast, broadcast::error::SendError};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::worker";

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
    info!(target: LOG_TARGET, "Start template sync for {transaction:?}"); // TODO: remove, only for testing

    // TODO: check method call as well if it works or needs syncing too

    // check for instructions
    for instruction in transaction.transaction.instructions() {
        info!(target: LOG_TARGET, "Current instruction: {instruction:?}..."); // TODO: remove, only for testing
        if let Instruction::CallFunction { template_address, .. } = instruction {
            info!(target: LOG_TARGET, "Template sync checking for {template_address}..."); // TODO: remove, only for testing
            tx_template_sync.send(TemplateSyncRequest::new(*template_address))?;
        }
    }

    Ok(())
}
