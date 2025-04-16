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
use tari_dan_common_types::SubstateLockType;
use tari_dan_storage::consensus_models::{BlockId, SubstateLock};
use tari_engine_types::substate::SubstateId;
use tari_transaction::TransactionId;

use crate::{
    codecs::{BlockIdCodec, DefaultCodec, SubstateIdCodec, SubstateLockKeyCodec, TransactionIdCodec, UnitCodec},
    traits::{Cf, QueryCf},
};

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct SubstateLockKey {
    pub block_id: BlockId,
    pub substate_id: SubstateId,
    pub transaction_id: TransactionId,
}

impl Display for SubstateLockKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SubstateLockKey {{ block_id: {}, substate_id: {}, transaction_id: {} }}",
            self.block_id, self.substate_id, self.transaction_id
        )
    }
}

pub struct SubstateLockModel;

impl Cf for SubstateLockModel {
    type Key = SubstateLockKey;
    type KeyCodec = SubstateLockKeyCodec<(TransactionId, SubstateId, BlockId)>;
    type Value = SubstateLock;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "locks"
    }
}

pub struct HeadIndex;

impl Cf for HeadIndex {
    type Key = SubstateId;
    type KeyCodec = SubstateIdCodec;
    type Value = SubstateLockKey;
    type ValueCodec = SubstateLockKeyCodec<(TransactionId, SubstateId, BlockId)>;

    fn name() -> &'static str {
        "locks_head_idx"
    }
}

pub struct ByTransactionIdQuery;

impl QueryCf for ByTransactionIdQuery {
    type Cf = SubstateLockModel;
    type Key = TransactionId;
    type KeyCodec = TransactionIdCodec;
}

pub struct BlockIdIndex;

impl Cf for BlockIdIndex {
    type Key = SubstateLockKey;
    type KeyCodec = SubstateLockKeyCodec<(BlockId, SubstateId, TransactionId)>;
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        "locks_block_id_idx"
    }
}

pub struct ByBlockIdQuery;

impl QueryCf for ByBlockIdQuery {
    type Cf = BlockIdIndex;
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
}

pub struct SubstateIdIndex;

impl Cf for SubstateIdIndex {
    type Key = SubstateLockKey;
    type KeyCodec = SubstateLockKeyCodec<(SubstateId, TransactionId, BlockId)>;
    type Value = SubstateLockType;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "locks_substate_id_idx"
    }
}

pub struct BySubstateIdQuery;

impl QueryCf for BySubstateIdQuery {
    type Cf = SubstateIdIndex;
    type Key = SubstateId;
    type KeyCodec = SubstateIdCodec;
}
