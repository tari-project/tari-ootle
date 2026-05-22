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

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_consensus_types::BlockId;
use tari_ootle_common_types::Epoch;
use tari_ootle_storage::consensus_models::ForeignProposalRecord;

use crate::{
    codecs::{BlockIdCodec, DefaultCodec, EpochCodec, KeyPrefix, UnitCodec},
    column_families::cf_names,
    prefixed,
    traits::{Cf, QueryCf},
};

prefixed!(ForeignProposalPrefix, KeyPrefix::ForeignProposals);
pub struct ForeignProposalCf;

impl Cf for ForeignProposalCf {
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
    type Prefix = ForeignProposalPrefix;
    type Value = ForeignProposalRecord;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::FOREIGN_PROPOSALS
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct ForeignProposalEpochIndexData {
    #[n(0)]
    pub block_id: BlockId,
    #[n(1)]
    pub proposed_in_block: Option<BlockId>,
}

prefixed!(ForeignProposalEpochIndexPrefix, KeyPrefix::ForeignProposalsEpochIndex);

// CF to query proposals by block epoch and status
pub struct EpochIndex;

impl Cf for EpochIndex {
    type Key = (Epoch, BlockId);
    type KeyCodec = (EpochCodec, BlockIdCodec);
    type Prefix = ForeignProposalEpochIndexPrefix;
    type Value = ForeignProposalEpochIndexData;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::FOREIGN_PROPOSALS
    }
}

/// Used to query proposals by epoch.
/// TODO: this is used to delete all FPs within an epoch. It feels like there is an efficient way to do with without a
/// separate CF
pub struct ByEpochQuery;

impl QueryCf for ByEpochQuery {
    type Cf = EpochIndex;
    type Key = Epoch;
    type KeyCodec = EpochCodec;
}

prefixed!(
    ProposedInBlockIndexPrefix,
    KeyPrefix::ForeignProposalsProposedInBlockIndex
);

/// CF to query proposals by the block_id they were proposed by
pub struct ProposedInBlockIndex;

impl Cf for ProposedInBlockIndex {
    // (proposed_in_block, block_id)
    type Key = (BlockId, BlockId);
    type KeyCodec = (BlockIdCodec, BlockIdCodec);
    type Prefix = ProposedInBlockIndexPrefix;
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        cf_names::FOREIGN_PROPOSALS
    }
}

pub struct ByProposedInBlockIndexQuery;

impl QueryCf for ByProposedInBlockIndexQuery {
    type Cf = ProposedInBlockIndex;
    // proposed_in_block
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
}

prefixed!(UnconfirmedIndexPrefix, KeyPrefix::ForeignProposalsUnconfirmedIndex);

/// CF that indexes unconfirmed foreign proposals
pub struct UnconfirmedIndex;

impl Cf for UnconfirmedIndex {
    type Key = (Epoch, BlockId);
    type KeyCodec = (EpochCodec, BlockIdCodec);
    type Prefix = UnconfirmedIndexPrefix;
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        cf_names::FOREIGN_PROPOSALS
    }
}

pub struct UnconfirmedIndexEpochQuery;

impl QueryCf for UnconfirmedIndexEpochQuery {
    type Cf = UnconfirmedIndex;
    type Key = Epoch;
    type KeyCodec = EpochCodec;
}
