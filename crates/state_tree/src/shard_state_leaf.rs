//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::{
    ProtocolVersion,
    hashing::{EngineHashDomainLabel, hasher32},
};
use tari_jellyfish::{TreeHash, Version};

/// The leaf a shard contributes to its shard group's state merkle root, the root a block header commits to.
///
/// From [`ProtocolVersion::V1`] the leaf commits to the shard's state version as well as its root, so the quorum
/// signature over a header fixes the version each shard of the group is at. Under [`ProtocolVersion::V0`] the leaf is
/// the shard root itself.
pub fn shard_state_leaf(protocol_version: ProtocolVersion, shard_root: &TreeHash, state_version: Version) -> TreeHash {
    match protocol_version {
        ProtocolVersion::V0 => *shard_root,
        ProtocolVersion::V1 | ProtocolVersion::V2 | ProtocolVersion::V3 => {
            let hash = hasher32(EngineHashDomainLabel::ShardStateLeaf)
                .chain(&shard_root.into_array())
                .chain(&state_version)
                .result();
            TreeHash::new(hash.into_array())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v0_leaf_is_the_shard_root() {
        let root = TreeHash::new([1u8; 32]);
        assert_eq!(shard_state_leaf(ProtocolVersion::V0, &root, 7), root);
    }

    #[test]
    fn v1_leaf_commits_to_the_state_version() {
        let root = TreeHash::new([1u8; 32]);
        let leaf = shard_state_leaf(ProtocolVersion::V1, &root, 7);
        assert_ne!(leaf, root);
        assert_ne!(leaf, shard_state_leaf(ProtocolVersion::V1, &root, 8));
        assert_ne!(
            leaf,
            shard_state_leaf(ProtocolVersion::V1, &TreeHash::new([2u8; 32]), 7)
        );
    }
}
