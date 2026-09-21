//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use serde::Serialize;
use tari_consensus_types::BlockId;
use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::TransactionId;

/// Most transactions one request may ask for. A request is decoded and buffered before consensus reaches the
/// view it names, so the limit belongs at the point the message is built from the wire: beyond it, the request
/// is not one an honest peer makes and its payload is not one this node holds.
pub const MAX_REQUESTED_TRANSACTIONS: usize = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct MissingTransactionsRequest {
    pub request_id: u32,
    pub epoch: Epoch,
    pub block_id: BlockId,
    pub transactions: HashSet<TransactionId>,
}
