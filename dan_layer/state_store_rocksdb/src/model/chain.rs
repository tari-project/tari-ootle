//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::consensus_models::BlockId;

use crate::{
    codecs::{BlockIdCodec, UnitCodec},
    traits::{Cf, QueryCf},
};

pub struct PendingChainIndex;

impl Cf for PendingChainIndex {
    // Child
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
    // Parent
    type Value = BlockId;
    type ValueCodec = BlockIdCodec;

    fn name() -> &'static str {
        "pending_chain"
    }
}

pub struct PendingParentChildIndex;
impl Cf for PendingParentChildIndex {
    // (Parent, Child)
    type Key = (BlockId, BlockId);
    type KeyCodec = (BlockIdCodec, BlockIdCodec);
    // Parent
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        "pending_parent_child_idx"
    }
}

pub struct ByParentIdQuery;

impl QueryCf for ByParentIdQuery {
    type Cf = PendingParentChildIndex;
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
}

/// This indexes the parent->child relationship of committed blocks in the chain.
// NOTE: Only needed for block sync and used in transaction_executions_get_pending_for_block. We can probably remove
// this by improving how block sync fetches blocks in batches.
pub struct CommittedParentChildChainIndex;

impl Cf for CommittedParentChildChainIndex {
    // Parent
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
    // Child
    type Value = BlockId;
    type ValueCodec = BlockIdCodec;

    fn name() -> &'static str {
        "committed_chain_idx"
    }
}
