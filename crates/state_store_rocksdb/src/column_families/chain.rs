//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::BlockId;

use crate::{
    codecs::{BlockIdCodec, KeyPrefix, UnitCodec},
    column_families::cf_names,
    prefixed,
    traits::{Cf, QueryCf},
};

prefixed!(PendingChainIndexPrefix, KeyPrefix::PendingChainIndex);

pub struct PendingChainIndex;

impl Cf for PendingChainIndex {
    // Child
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
    type Prefix = PendingChainIndexPrefix;
    // Parent
    type Value = BlockId;
    type ValueCodec = BlockIdCodec;

    fn name() -> &'static str {
        cf_names::CHAIN_METADATA
    }
}

prefixed!(PendingParentChildIndexPrefix, KeyPrefix::PendingParentChildIndex);

pub struct PendingParentChildIndex;
impl Cf for PendingParentChildIndex {
    // (Parent, Child)
    type Key = (BlockId, BlockId);
    type KeyCodec = (BlockIdCodec, BlockIdCodec);
    type Prefix = PendingParentChildIndexPrefix;
    // Parent
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        cf_names::CHAIN_METADATA
    }
}

pub struct ByParentIdQuery;

impl QueryCf for ByParentIdQuery {
    type Cf = PendingParentChildIndex;
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
}

prefixed!(CommittedParentChildIndexPrefix, KeyPrefix::CommittedParentChildIndex);

/// This indexes the parent->child relationship of committed blocks in the chain.
// NOTE: Only needed for block sync and used in transaction_executions_get_pending_for_block. We can probably remove
// this by improving how block sync fetches blocks in batches.
pub struct CommittedParentChildChainIndex;

impl Cf for CommittedParentChildChainIndex {
    // Parent
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
    type Prefix = CommittedParentChildIndexPrefix;
    // Child
    type Value = BlockId;
    type ValueCodec = BlockIdCodec;

    fn name() -> &'static str {
        cf_names::CHAIN_METADATA
    }
}
