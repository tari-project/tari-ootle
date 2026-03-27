//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::BlockId;
use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_ootle_storage::consensus_models::Block;

use crate::{
    codecs::{BlockIdCodec, DefaultCodec, EpochCodec, KeyPrefix, NodeHeightCodec, UnitCodec},
    column_families::cf_names,
    prefixed,
    traits::{Cf, QueryCf},
};

prefixed!(BlockPrefix, KeyPrefix::Blocks);

pub struct BlockCf;

impl Cf for BlockCf {
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
    type Prefix = BlockPrefix;
    type Value = Block;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::BLOCK
    }
}

prefixed!(BlockEpochHeightPrefix, KeyPrefix::BlockEpochHeightIndex);

pub struct EpochHeightIndex;

impl Cf for EpochHeightIndex {
    type Key = (Epoch, NodeHeight, BlockId);
    type KeyCodec = (EpochCodec, NodeHeightCodec, BlockIdCodec);
    type Prefix = BlockEpochHeightPrefix;
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        BlockCf::name()
    }
}

pub struct ByEpochHeightQuery;

impl QueryCf for ByEpochHeightQuery {
    type Cf = EpochHeightIndex;
    type Key = (Epoch, NodeHeight);
    type KeyCodec = (EpochCodec, NodeHeightCodec);
}

pub struct ByEpochQuery;

impl QueryCf for ByEpochQuery {
    type Cf = EpochHeightIndex;
    type Key = Epoch;
    type KeyCodec = EpochCodec;
}
