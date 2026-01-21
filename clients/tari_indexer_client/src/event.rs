//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::transaction_receipt::FinalizeOutcome;
use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::TransactionId;

#[derive(Debug, Clone)]
pub enum IndexerEvent {
    NewEpoch(NewEpochEvent),
    TransactionFinalized(TransactionFinalizedEvent),
}

impl IndexerEvent {
    pub const fn as_event_name(&self) -> &'static str {
        match self {
            Self::NewEpoch(_) => "NewEpoch",
            Self::TransactionFinalized(_) => "TransactionFinalized",
        }
    }
}

impl From<NewEpochEvent> for IndexerEvent {
    fn from(event: NewEpochEvent) -> Self {
        Self::NewEpoch(event)
    }
}

impl From<TransactionFinalizedEvent> for IndexerEvent {
    fn from(event: TransactionFinalizedEvent) -> Self {
        Self::TransactionFinalized(event)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewEpochEvent {
    pub epoch: Epoch,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionFinalizedEvent {
    pub transaction_id: TransactionId,
    pub outcome: FinalizeOutcome,
}
