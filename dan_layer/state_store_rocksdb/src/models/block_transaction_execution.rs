//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::NodeHeight;
use tari_dan_storage::consensus_models::{BlockId, BlockTransactionExecution};
use tari_transaction::TransactionId;

use crate::{
    codecs::{BlockIdCodec, DefaultCodec, NumberCodec, TransactionIdCodec, UnitCodec},
    traits::{Cf, QueryCf},
};

pub struct BlockTransactionExecutionModel;

impl Cf for BlockTransactionExecutionModel {
    type Key = (TransactionId, BlockId, NodeHeight);
    type KeyCodec = (TransactionIdCodec, BlockIdCodec, NumberCodec<NodeHeight>);
    type Value = BlockTransactionExecution;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "block_transaction_exec"
    }
}

pub struct ByTransactionIdQuery;

impl QueryCf for ByTransactionIdQuery {
    type Cf = BlockTransactionExecutionModel;
    type Key = TransactionId;
    type KeyCodec = TransactionIdCodec;
}

pub struct BlockIndex;

impl Cf for BlockIndex {
    type Key = (BlockId, TransactionId, NodeHeight);
    type KeyCodec = (BlockIdCodec, TransactionIdCodec, NumberCodec<NodeHeight>);
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        "block_transaction_exec_block_idx"
    }
}

pub struct ByBlockQuery;

impl QueryCf for ByBlockQuery {
    type Cf = BlockIndex;
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
}

pub struct ByBlockAndTransactionQuery;

impl QueryCf for ByBlockAndTransactionQuery {
    type Cf = BlockIndex;
    type Key = (BlockId, TransactionId);
    type KeyCodec = (BlockIdCodec, TransactionIdCodec);
}
