//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use serde::Serialize;
use tari_consensus_types::BlockId;
use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::TransactionId;

#[derive(Debug, Clone, Serialize)]
pub struct MissingTransactionsRequest {
    pub request_id: u32,
    pub epoch: Epoch,
    pub block_id: BlockId,
    pub transactions: HashSet<TransactionId>,
}
