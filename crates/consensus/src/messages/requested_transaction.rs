//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::Serialize;
use tari_consensus_types::BlockId;
use tari_ootle_common_types::Epoch;
use tari_transaction::Transaction;

#[derive(Debug, Clone, Serialize)]
pub struct MissingTransactionsResponse {
    pub request_id: u32,
    pub epoch: Epoch,
    pub block_id: BlockId,
    pub transactions: Vec<Transaction>,
}
