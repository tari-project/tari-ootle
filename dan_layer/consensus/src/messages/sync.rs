//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::Serialize;
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_dan_storage::consensus_models::{Block, QuorumCertificate};
use tari_transaction::Transaction;

#[derive(Debug, Clone, Serialize)]
pub struct SyncRequestMessage {
    pub epoch: Epoch,
    pub block_height: NodeHeight,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncResponseMessage {
    pub epoch: Epoch,
    pub blocks: Vec<FullBlock>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FullBlock {
    pub block: Block,
    pub qcs: Vec<QuorumCertificate>,
    pub transactions: Vec<Transaction>,
}
