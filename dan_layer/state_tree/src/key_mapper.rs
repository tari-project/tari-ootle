//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::VersionedSubstateId;
use tari_jellyfish::{jmt_node_hash, LeafKey, TreeHash};

pub trait DbKeyMapper<T> {
    fn map_to_leaf_key(id: &T) -> LeafKey;
}

pub struct SpreadPrefixKeyMapper;

impl DbKeyMapper<VersionedSubstateId> for SpreadPrefixKeyMapper {
    fn map_to_leaf_key(id: &VersionedSubstateId) -> LeafKey {
        let hash = jmt_node_hash(id);
        LeafKey::new(hash)
    }
}

pub struct HashIdentityKeyMapper;

impl DbKeyMapper<TreeHash> for HashIdentityKeyMapper {
    fn map_to_leaf_key(hash: &TreeHash) -> LeafKey {
        LeafKey::new(*hash)
    }
}
