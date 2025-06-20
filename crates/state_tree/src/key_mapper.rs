//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use digest::consts;
use jmt::KeyHash;
use tari_crypto::hash_domain;
use tari_hashing::layer2::{tari_hasher32, TariDomainHasher};
use tari_ootle_common_types::VersionedSubstateId;

pub trait DbKeyMapper<T> {
    fn map_to_leaf_key(id: &T) -> KeyHash;
}

pub struct SpreadPrefixKeyMapper;

impl DbKeyMapper<VersionedSubstateId> for SpreadPrefixKeyMapper {
    fn map_to_leaf_key(id: &VersionedSubstateId) -> KeyHash {
        let hash = jmt_versioned_substate_hasher().chain(&id).finalize_into_array();
        KeyHash(hash)
    }
}

pub struct HashIdentityKeyMapper;

impl DbKeyMapper<KeyHash> for HashIdentityKeyMapper {
    fn map_to_leaf_key(hash: &KeyHash) -> KeyHash {
        *hash
    }
}

hash_domain!(JmtSubstateHashDomain, "com.tari.jmt", 0);

fn jmt_versioned_substate_hasher() -> TariDomainHasher<JmtSubstateHashDomain, consts::U32> {
    tari_hasher32::<JmtSubstateHashDomain>("VersionedSubstateId")
}
