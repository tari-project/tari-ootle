//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::BlockId;
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_dan_storage::consensus_models::Block;

use crate::{
    codecs::{BlockIdCodec, DefaultCodec, EpochCodec, NodeHeightCodec, UnitCodec},
    traits::{Cf, QueryCf},
};

pub struct BlockCf;

impl Cf for BlockCf {
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
    type Value = Block;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "blocks"
    }
}

pub struct EpochHeightIndex;

impl Cf for EpochHeightIndex {
    type Key = (Epoch, NodeHeight, BlockId);
    type KeyCodec = (EpochCodec, NodeHeightCodec, BlockIdCodec);
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        "block_height_idx"
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
