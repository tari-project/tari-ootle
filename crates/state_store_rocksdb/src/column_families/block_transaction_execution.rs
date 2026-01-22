//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::BlockId;
use tari_ootle_common_types::NodeHeight;
use tari_ootle_storage::consensus_models::BlockTransactionExecution;
use tari_ootle_transaction::TransactionId;

use crate::{
    codecs::{BlockIdCodec, DefaultVersionedCodec, KeyPrefix, NumberCodec, TransactionIdCodec, UnitCodec},
    column_families::block::BlockCf,
    prefixed,
    traits::{Cf, QueryCf},
    versioned_types::VersionedBlockTransactionExecution,
};

prefixed!(BlockTransactionExecutionPrefix, KeyPrefix::BlockTransactionExecutions);

pub struct BlockTransactionExecutionCf;

impl Cf for BlockTransactionExecutionCf {
    // The node height is included so that executions can be filtered by height in
    // block_transaction_executions_get_pending_for_block.
    type Key = (TransactionId, BlockId, NodeHeight);
    type KeyCodec = (TransactionIdCodec, BlockIdCodec, NumberCodec<NodeHeight>);
    type Prefix = BlockTransactionExecutionPrefix;
    type Value = BlockTransactionExecution;
    type ValueCodec = DefaultVersionedCodec<VersionedBlockTransactionExecution>;

    fn name() -> &'static str {
        BlockCf::name()
    }
}

pub struct ByTransactionIdQuery;

impl QueryCf for ByTransactionIdQuery {
    type Cf = BlockTransactionExecutionCf;
    type Key = TransactionId;
    type KeyCodec = TransactionIdCodec;
}

prefixed!(
    BlockTransactionExecutionIndexPrefix,
    KeyPrefix::BlockTransactionExecutionByBlockIdIndex
);

pub struct BlockIndex;

impl Cf for BlockIndex {
    type Key = (BlockId, TransactionId, NodeHeight);
    type KeyCodec = (BlockIdCodec, TransactionIdCodec, NumberCodec<NodeHeight>);
    type Prefix = BlockTransactionExecutionIndexPrefix;
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        BlockCf::name()
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
