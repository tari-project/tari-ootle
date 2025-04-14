//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::consensus_models::{BlockId, BlockTransactionExecution};
use tari_transaction::TransactionId;

use crate::{
    codecs::{BlockIdCodec, DefaultCodec, TransactionIdCodec, TupleBytesCodec, UnitCodec},
    traits::{Cf, QueryCf},
};

pub struct BlockTransactionExecutionModel;

impl Cf for BlockTransactionExecutionModel {
    type Key = (BlockId, TransactionId);
    type KeyCodec = TupleBytesCodec<Self::Key>;
    type Value = BlockTransactionExecution;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "transaction_exec"
    }
}

pub struct ByBlockQuery;

impl QueryCf for ByBlockQuery {
    type Cf = BlockTransactionExecutionModel;
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
}

pub struct TransactionIndex;

impl Cf for TransactionIndex {
    type Key = (TransactionId, BlockId);
    type KeyCodec = (TransactionIdCodec, BlockIdCodec);
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        "transaction_exec_tx_idx"
    }
}

pub struct ByTransactionIdQuery;

impl QueryCf for ByTransactionIdQuery {
    type Cf = TransactionIndex;
    type Key = TransactionId;
    type KeyCodec = TransactionIdCodec;
}
