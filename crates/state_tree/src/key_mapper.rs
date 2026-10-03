//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_jellyfish::{LeafKey, TreeHash, jmt_node_hash};
use tari_ootle_common_types::{VersionedSubstateId, shard::Shard};

pub trait DbKeyMapper<T> {
    fn map_to_leaf_key(id: &T) -> LeafKey;
}

/// A key mapper that "spreads" keys over the key space by hashing the VersionedSubstateId.
/// This is done for performance reasons to "spead" the tree out rather than potentially having branches with a high
/// depth.
pub struct SpreadPrefixKeyMapper;

impl DbKeyMapper<VersionedSubstateId> for SpreadPrefixKeyMapper {
    fn map_to_leaf_key(id: &VersionedSubstateId) -> LeafKey {
        let hash = jmt_node_hash(id);
        LeafKey::new(hash)
    }
}

/// A key mapper that uses the hash directly as the leaf key.
/// This is useful if the key is already a pseudo-random hash and does not need further spreading.
pub struct HashIdentityKeyMapper;

impl DbKeyMapper<TreeHash> for HashIdentityKeyMapper {
    fn map_to_leaf_key(hash: &TreeHash) -> LeafKey {
        LeafKey::new(*hash)
    }
}

/// A key mapper that keys a leaf by the shard it belongs to, so that a tree over per-shard values
/// commits to which shard each value belongs to.
pub struct ShardKeyMapper;

impl DbKeyMapper<Shard> for ShardKeyMapper {
    fn map_to_leaf_key(shard: &Shard) -> LeafKey {
        let hash = jmt_node_hash(&shard.as_u32());
        LeafKey::new(hash)
    }
}
