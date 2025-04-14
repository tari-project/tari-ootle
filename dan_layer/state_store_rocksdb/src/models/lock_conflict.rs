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

use tari_dan_storage::consensus_models::{BlockId, LockConflict};
use tari_transaction::TransactionId;

use crate::{
    codecs::{BlockIdCodec, DefaultCodec, TransactionIdCodec, TupleBytesCodec, UnitCodec},
    traits::{Cf, QueryCf},
};

pub struct LockConflictModel;

impl Cf for LockConflictModel {
    // (transaction id, Block, depends_on_tx_id)
    type Key = (TransactionId, BlockId, TransactionId);
    type KeyCodec = TupleBytesCodec<Self::Key>;
    type Value = LockConflict;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "lockconflicts"
    }
}

pub struct ByTransactionIdQuery;

impl QueryCf for ByTransactionIdQuery {
    type Cf = LockConflictModel;
    type Key = TransactionId;
    type KeyCodec = TransactionIdCodec;
}

pub struct LockConflictBlockIdIndex;

impl Cf for LockConflictBlockIdIndex {
    // (block id, transaction id, depends_on_tx_id)
    type Key = (BlockId, TransactionId, TransactionId);
    type KeyCodec = TupleBytesCodec<Self::Key>;
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        // Same column family
        "lockconflicts"
    }
}

pub struct ByBlockIdQuery;

impl QueryCf for ByBlockIdQuery {
    type Cf = LockConflictBlockIdIndex;
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
}
