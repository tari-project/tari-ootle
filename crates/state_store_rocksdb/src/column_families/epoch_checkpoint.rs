//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{Epoch, ShardGroup};
use tari_ootle_storage::consensus_models::EpochCheckpoint;

use crate::{
    codecs::{DefaultVersionedCodec, EpochCodec, KeyPrefix, ShardGroupCodec},
    column_families::cf_names,
    prefixed,
    traits::Cf,
    versioned_types::VersionedEpochCheckpoint,
};

prefixed!(EpochCheckpointPrefix, KeyPrefix::EpochCheckpoints);
pub struct EpochCheckpointCf;

impl Cf for EpochCheckpointCf {
    type Key = (Epoch, ShardGroup);
    type KeyCodec = (EpochCodec, ShardGroupCodec);
    type Prefix = EpochCheckpointPrefix;
    type Value = EpochCheckpoint;
    type ValueCodec = DefaultVersionedCodec<VersionedEpochCheckpoint>;

    fn name() -> &'static str {
        cf_names::CHAIN_METADATA
    }
}

pub struct ByEpochQuery;

impl crate::traits::QueryCf for ByEpochQuery {
    type Cf = EpochCheckpointCf;
    type Key = Epoch;
    type KeyCodec = EpochCodec;
}
