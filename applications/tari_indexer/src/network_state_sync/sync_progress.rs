//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, Seq};
use tari_ootle_common_types::{shard::Shard, Epoch, ShardGroup, StateVersion};

#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncProgress {
    pub last_epoch: Epoch,
    // These transform the maps into Vecs for JSON serialization (can't have non-string keys).
    #[serde_as(as = "Seq<(_, _)>")]
    pub checkpoint_progress: IndexMap<ShardGroup, Epoch>,
    #[serde_as(as = "Seq<(_, _)>")]
    pub last_state_versions: IndexMap<Shard, (StateVersion, Epoch)>,
}
