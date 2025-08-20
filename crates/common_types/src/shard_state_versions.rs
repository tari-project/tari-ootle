//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use bounded_vec::BoundedVec;
pub use bounded_vec::BoundedVecOutOfBounds;
use tari_bor::{Deserialize, Serialize};

use crate::{shard::Shard, NumPreshards, ShardGroup};

/// Maximum number of shards is one more than the maximum number of presharding options to allow for the global shard
const MAX_SHARDS: usize = NumPreshards::MAX_SHARD.as_u32() as usize + 1;

type BoundedVersionVec = BoundedVec<u64, 1, MAX_SHARDS>;

/// The state versions for each shard that maps each shard managed by the ShardGroup (including the
/// global shard) to a state version.
///
/// For example, if the ShardGroup is [1, 3], the state versions will contain 4
/// elements. The first element is always the global shard (shard 0) version. The second element is the state
/// version for shard 1, third is shard 2, and forth is shard 3.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
#[serde(transparent)]
pub struct ShardStateVersions {
    #[cfg_attr(feature = "ts", ts(type = "number[]"))]
    inner: BoundedVersionVec,
}

impl ShardStateVersions {
    pub fn genesis(shard_group: ShardGroup) -> Self {
        Self {
            inner: BoundedVersionVec::try_from(vec![0; shard_group.len() + 1])
                .expect("Empty vec should always be valid"),
        }
    }

    pub fn from_vec(shard_versions: Vec<u64>) -> Result<Self, BoundedVecOutOfBounds> {
        Ok(Self {
            inner: BoundedVersionVec::from_vec(shard_versions)?,
        })
    }

    pub fn into_vec(self) -> Vec<u64> {
        self.inner.into()
    }

    pub fn get(&self, shard_index: usize) -> Option<u64> {
        self.inner.get(shard_index).copied()
    }

    pub fn get_global(&self) -> u64 {
        *self.inner.first()
    }

    pub fn shard_to_index(shard_group: ShardGroup, shard: Shard) -> Option<usize> {
        if shard.is_global() {
            return Some(0);
        }

        if !shard_group.contains_or_global(&shard) {
            return None;
        }
        shard_group.checked_len()?;
        if shard_group.end().as_u32() as usize > MAX_SHARDS {
            return None;
        }
        let index = shard.as_u32().checked_sub(shard_group.start().as_u32())? as usize;
        // + 1 to account for the global shard at index 0
        Some(index + 1)
    }

    pub fn get_by_shard_checked(&self, shard_group: ShardGroup, shard: Shard) -> Option<u64> {
        let index = Self::shard_to_index(shard_group, shard)?;
        self.inner.get(index).copied()
    }

    pub fn len(&self) -> usize {
        self.inner.as_slice().len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn as_slice(&self) -> &[u64] {
        self.inner.as_slice()
    }

    pub fn apply_bitmap(mut self, bitmap: Vec<bool>) -> Self {
        if self.len() != bitmap.len() {
            panic!("Length mismatch: expected {} but got {}", self.len(), bitmap.len());
        }

        let inner_mut: &mut [u64] = self.inner.as_mut();
        for (i, _) in bitmap.into_iter().enumerate().filter(|(_, v)| *v) {
            inner_mut[i] += 1;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_gets_by_index() {
        let versions = ShardStateVersions::from_vec(vec![1, 2, 3]).unwrap();
        assert_eq!(versions.get(0), Some(1));
        assert_eq!(versions.get(1), Some(2));
        assert_eq!(versions.get(2), Some(3));
        assert_eq!(versions.get(3), None);
        assert_eq!(versions.len(), 3);
        assert!(!versions.is_empty());
    }

    #[test]
    fn it_gets_by_shard() {
        let v = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];
        let num_non_global_shards = v.len() as u32 - 1;
        let versions = ShardStateVersions::from_vec(v).unwrap();
        let shard_group = ShardGroup::new(100, 100 + num_non_global_shards);
        assert_eq!(shard_group.len(), versions.len());
        let v = versions.get_by_shard_checked(shard_group, 0.into()).unwrap();
        assert_eq!(v, 1, "returned incorrect version {v} for global shard");

        for (i, shard) in (100..100 + num_non_global_shards).enumerate() {
            let v = versions
                .get_by_shard_checked(shard_group, shard.into())
                .unwrap_or_else(|| panic!("Shard {} not found", shard));
            assert_eq!(v, i as u64 + 2, "returned incorrect version {v} for shard {}", shard);
        }
        let v = versions.get_by_shard_checked(shard_group, Shard::from(13));
        assert!(v.is_none());
    }

    #[test]
    fn it_errors_if_more_then_max_shards() {
        let e = ShardStateVersions::from_vec(vec![1; MAX_SHARDS + 1]).unwrap_err();
        assert!(matches!(e, BoundedVecOutOfBounds::UpperBoundError { .. }));
    }

    #[test]
    fn it_deserializes_if_serialized_vec_is_within_bounds() {
        let v = vec![1, 2, 3];
        let serialized = tari_bor::encode(&v).unwrap();
        let deserialized: ShardStateVersions = tari_bor::decode(&serialized).unwrap();
        assert_eq!(deserialized.as_slice(), v);
    }

    #[test]
    fn it_errors_if_serialized_vec_is_empty() {
        let v = Vec::<u64>::new();
        let serialized = tari_bor::encode(&v).unwrap();
        tari_bor::decode::<ShardStateVersions>(&serialized).unwrap_err();
    }

    #[test]
    fn it_errors_if_serialized_vec_is_too_large() {
        let v = vec![1; MAX_SHARDS + 1];
        let serialized = tari_bor::encode(&v).unwrap();
        tari_bor::decode::<ShardStateVersions>(&serialized).unwrap_err();
    }

    #[test]
    fn it_applies_a_bitmap_to_increment_versions() {
        let versions = ShardStateVersions::from_vec(vec![1, 2, 3]).unwrap();
        let bitmap = vec![true, false, true];
        let updated_versions = versions.apply_bitmap(bitmap);

        assert_eq!(updated_versions.get(0), Some(2));
        assert_eq!(updated_versions.get(1), Some(2));
        assert_eq!(updated_versions.get(2), Some(4));
        assert_eq!(updated_versions.len(), 3);
    }
}
