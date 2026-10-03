//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::shard::Shard;
use tari_state_tree::Version;

use crate::{
    codecs::{KeyPrefix, NumberCodec, ShardCodec},
    column_families::cf_names,
    prefixed,
    traits::Cf,
};

prefixed!(StateSyncRewindPointPrefix, KeyPrefix::StateSyncRewindPoint);

/// Per shard, the version state sync rewinds to if the versions it committed above it are never verified.
pub struct StateSyncRewindPointCf;

impl Cf for StateSyncRewindPointCf {
    type Key = Shard;
    type KeyCodec = ShardCodec;
    type Prefix = StateSyncRewindPointPrefix;
    type Value = Version;
    type ValueCodec = NumberCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::STATE_TREE
    }
}
