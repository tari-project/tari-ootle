//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_dan_storage::consensus_models::{BlockId, SubstateChange};
use tari_engine_types::substate::SubstateId;

use crate::{
    codecs::{
        BlockDiffKeyCodec,
        BlockIdCodec,
        BlockIdSubstateIdVersionCodec,
        DefaultCodec,
        DefaultCodecRef,
        SubstateIdCodec,
        UnitCodec,
    },
    traits::{Cf, QueryCf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDiffEntry {
    pub block_id: BlockId,
    pub substate_id: SubstateId,
    pub change: SubstateChange,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockDiffInsertEntry<'a> {
    pub substate_id: &'a SubstateId,
    pub change: &'a SubstateChange,
}

pub struct BlockDiffKey {
    pub block_id: BlockId,
    pub substate_id: SubstateId,
    pub version: u32,
}
pub struct BlockDiffModel;

impl Cf for BlockDiffModel {
    type Key = BlockDiffKey;
    type KeyCodec = BlockIdSubstateIdVersionCodec;
    type Value = SubstateChange;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "block_diff"
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockDiffRef<'a> {
    pub change: &'a SubstateChange,
}

#[derive(Default)]
pub struct BlockDiffModelRef<'a> {
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> Cf for BlockDiffModelRef<'a> {
    type Key = <BlockDiffModel as Cf>::Key;
    type KeyCodec = <BlockDiffModel as Cf>::KeyCodec;
    type Value = BlockDiffRef<'a>;
    type ValueCodec = DefaultCodecRef<Self::Value>;

    fn name() -> &'static str {
        BlockDiffModel::name()
    }
}

pub struct ByBlockIdQuery;

impl QueryCf for ByBlockIdQuery {
    type Cf = BlockDiffModel;
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
}

pub struct SubstateIdIndex;

impl Cf for SubstateIdIndex {
    type Key = BlockDiffKey;
    type KeyCodec = BlockDiffKeyCodec;
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        "block_diff_substate_id_idx"
    }
}

pub struct BySubstateIdQuery;

impl QueryCf for BySubstateIdQuery {
    type Cf = SubstateIdIndex;
    type Key = SubstateId;
    type KeyCodec = SubstateIdCodec;
}
