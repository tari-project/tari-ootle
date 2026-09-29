//  Copyright 2025. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::fmt::Display;

use serde::Serialize;
use tari_consensus_types::BlockId;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{NodeHeight, SubstateLockType};
use tari_ootle_storage::consensus_models::SubstateLock;
use tari_ootle_transaction::TransactionId;

use crate::{
    codecs::{
        BlockIdCodec,
        DefaultCodec,
        KeyPrefix,
        NodeHeightCodec,
        NumberCodec,
        SubstateIdCodec,
        SubstateLockKeyCodec,
        TransactionIdCodec,
    },
    column_families::cf_names,
    prefixed,
    traits::{Cf, QueryCf},
};

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct SubstateLockKey {
    pub block_id: BlockId,
    pub block_height: NodeHeight,
    pub substate_id: SubstateId,
    pub transaction_id: TransactionId,
}

impl Display for SubstateLockKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SubstateLockKey {{ block: {}/{}, substate_id: {}, transaction_id: {} }}",
            self.block_height, self.block_id, self.substate_id, self.transaction_id
        )
    }
}

prefixed!(SubstateLockPrefix, KeyPrefix::SubstateLocks);

pub struct SubstateLockModel;

impl Cf for SubstateLockModel {
    type Key = SubstateLockKey;
    type KeyCodec = SubstateLockKeyCodec<(TransactionId, SubstateId, BlockId, NodeHeight)>;
    type Prefix = SubstateLockPrefix;
    type Value = SubstateLock;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::SUBSTATES
    }
}

prefixed!(SubstateLockChainOrderPrefix, KeyPrefix::SubstateLockChainOrderIndex);

/// Orders a substate's locks the way a chain grants them: by block height, then by the order the block granted them.
///
/// `grant_seq` is a lock's position in the sequence its block granted for the substate. A chain holds one block per
/// height, so `(block_height, grant_seq)` totally orders every lock a chain holds on the substate, and a descending
/// scan filtered to one chain's blocks yields its most recently granted lock first. `block_id` sits between the two so
/// that a block's entries can be deleted as one prefix range.
pub struct ChainOrderIndex;

impl Cf for ChainOrderIndex {
    type Key = (SubstateId, NodeHeight, BlockId, u32);
    type KeyCodec = (SubstateIdCodec, NodeHeightCodec, BlockIdCodec, NumberCodec<u32>);
    type Prefix = SubstateLockChainOrderPrefix;
    type Value = TransactionId;
    type ValueCodec = TransactionIdCodec;

    fn name() -> &'static str {
        cf_names::SUBSTATES
    }
}

/// Every lock held on a substate, most recently granted first when scanned descending.
pub struct ByChainOrderQuery;

impl QueryCf for ByChainOrderQuery {
    type Cf = ChainOrderIndex;
    type Key = SubstateId;
    type KeyCodec = SubstateIdCodec;
}

pub struct ByTransactionIdQuery;

impl QueryCf for ByTransactionIdQuery {
    type Cf = SubstateLockModel;
    type Key = TransactionId;
    type KeyCodec = TransactionIdCodec;
}

prefixed!(SubstatesBlockIdIndexPrefix, KeyPrefix::SubstateLocksBlockIdIndex);

pub struct BlockIdIndex;

/// The value is the lock's `grant_seq`, which completes its [`ChainOrderIndex`] key. Holding it here lets a lock be
/// removed from that index by exact key, since every path that removes a lock reaches it by block or by transaction.
impl Cf for BlockIdIndex {
    type Key = SubstateLockKey;
    type KeyCodec = SubstateLockKeyCodec<(BlockId, SubstateId, TransactionId, NodeHeight)>;
    type Prefix = SubstatesBlockIdIndexPrefix;
    type Value = u32;
    type ValueCodec = NumberCodec<u32>;

    fn name() -> &'static str {
        cf_names::SUBSTATES
    }
}

pub struct ByBlockIdQuery;

impl QueryCf for ByBlockIdQuery {
    type Cf = BlockIdIndex;
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
}

prefixed!(SubstateIdIndexPrefix, KeyPrefix::SubstateLockSubstateIdIndex);

pub struct SubstateIdIndex;

impl Cf for SubstateIdIndex {
    type Key = SubstateLockKey;
    type KeyCodec = SubstateLockKeyCodec<(SubstateId, TransactionId, BlockId, NodeHeight)>;
    type Prefix = SubstateIdIndexPrefix;
    type Value = SubstateLockType;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::SUBSTATES
    }
}

pub struct BySubstateIdQuery;

impl QueryCf for BySubstateIdQuery {
    type Cf = SubstateIdIndex;
    type Key = SubstateId;
    type KeyCodec = SubstateIdCodec;
}

/// The [`SubstateIdIndex`] key shape from before the index carried its table prefix.
///
/// These keys are namespaced only by the leading borsh `SubstateId` discriminant, so they sort below every prefixed
/// table sharing the column family. Exists so the migration that rewrites them can address them; remove it once no
/// supported database can still be at a version that predates that migration.
pub struct LegacyUnprefixedSubstateIdIndex;

impl Cf for LegacyUnprefixedSubstateIdIndex {
    type Key = SubstateLockKey;
    type KeyCodec = SubstateLockKeyCodec<(SubstateId, TransactionId, BlockId, NodeHeight)>;
    type Prefix = ();
    type Value = SubstateLockType;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::SUBSTATES
    }
}
