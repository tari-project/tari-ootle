//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use core::fmt;
use std::{
    fmt::{Display, Formatter},
    iter,
    ops::RangeInclusive,
    str::FromStr,
};

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};

use crate::{shard::Shard, uint::U256, NumPreshards, SubstateAddress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, BorshSerialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct ShardGroup {
    start: Shard,
    end_inclusive: Shard,
}

impl ShardGroup {
    const MAX_ENCODED_VALUE: u32 = (NumPreshards::MAX.as_u32() << 16) + NumPreshards::MAX.as_u32();

    /// Creates a new ShardGroup with the given start and end inclusive shards.
    /// ## Panics
    /// Panics if the start shard is greater than the end shard.
    pub fn new<T: Into<Shard> + Copy>(start: T, end_inclusive: T) -> Self {
        Self::new_checked(start, end_inclusive)
            .expect("INVARIANT: start shard must be less than or equal to end_inclusive")
    }

    pub fn new_checked<T: Into<Shard> + Copy>(start: T, end_inclusive: T) -> Option<Self> {
        let start = start.into();
        let end_inclusive = end_inclusive.into();
        if start > end_inclusive {
            return None;
        }
        Some(Self { start, end_inclusive })
    }

    /// Creates a new ShardGroup. The shard group bounds are not checked.
    /// Prepare checked_new unless the bounds have already been checked by the caller.
    pub fn new_unchecked<T: Into<Shard> + Copy>(start: T, end_inclusive: T) -> Self {
        Self {
            start: start.into(),
            end_inclusive: end_inclusive.into(),
        }
    }

    pub fn all_shards(num_preshards: NumPreshards) -> Self {
        Self::new(Shard::first(), Shard::from(num_preshards.as_u32()))
    }

    /// Returns the number of shards in the shard group.
    /// WARN: If the bounds are invalid this will panic/underflow.
    /// If this comes from an untrusted source, `checked_len` should be used to verify the bounds.
    pub const fn len(&self) -> usize {
        (self.end_inclusive.as_u32() + 1 - self.start.as_u32()) as usize
    }

    /// Returns the length of the shard group, or None if the bounds are invalid
    /// The minimum length returned is 1 since the bounds are inclusive.
    pub fn checked_len(&self) -> Option<usize> {
        let len = self
            .end_inclusive
            .as_u32()
            .checked_add(1)?
            .checked_sub(self.start.as_u32())?;
        Some(len as usize).filter(|len| *len > 0)
    }

    pub const fn is_empty(&self) -> bool {
        // Can never be empty because start <= end_inclusive (self.len() >= 1)
        false
    }

    /// Encodes the shard group as a u32. Big endian layout: (start_msb)(start_lsb)(end_msb)(end_lsb).
    /// The maximum shard number is 256 (0x100), so in practise start_msb and end_msb are either 1 or 0.
    pub fn encode_as_u32(&self) -> u32 {
        let mut n = self.start.as_u32() << 16;
        n |= self.end_inclusive.as_u32();
        n
    }

    pub fn decode_from_u32(n: u32) -> Option<Self> {
        if n > Self::MAX_ENCODED_VALUE {
            return None;
        }

        let start = n >> 16;
        let end = n & 0xFFFF;
        Self::new_checked(start, end)
    }

    pub fn shard_iter(self) -> impl Iterator<Item = Shard> + 'static {
        iter::successors(Some(self.start), move |&shard| {
            if shard == self.end_inclusive {
                None
            } else {
                Some(Shard::from(shard.as_u32() + 1))
            }
        })
    }

    pub fn start(&self) -> Shard {
        self.start
    }

    pub fn end(&self) -> Shard {
        self.end_inclusive
    }

    pub fn contains(&self, shard: &Shard) -> bool {
        self.as_range().contains(shard)
    }

    pub fn contains_or_global(&self, shard: &Shard) -> bool {
        if shard.is_global() {
            return true;
        }
        self.contains(shard)
    }

    pub fn overlaps_shard_group(&self, other: &ShardGroup) -> bool {
        self.start <= other.end_inclusive && self.end_inclusive >= other.start
    }

    pub fn as_range(&self) -> RangeInclusive<Shard> {
        self.start..=self.end_inclusive
    }

    pub fn to_substate_address_range(self, num_shards: NumPreshards) -> RangeInclusive<SubstateAddress> {
        if num_shards.is_one() {
            return SubstateAddress::zero()..=SubstateAddress::max();
        }

        let shard_size = U256::MAX >> num_shards.as_u32().trailing_zeros();
        let start = if self.start.is_first() {
            U256::ZERO
        } else {
            shard_size * U256::from(self.start.as_u32()) + U256::from(self.start.as_u32() - 1)
        };
        if self.end_inclusive == num_shards.as_u32() {
            return SubstateAddress::from_u256_zero_version(start)..=SubstateAddress::max();
        }

        let end =
            shard_size * U256::from(self.end_inclusive.as_u32()) + shard_size + U256::from(self.end_inclusive.as_u32());
        SubstateAddress::from_u256_zero_version(start)..=SubstateAddress::from_u256_zero_version(end - 1)
    }

    pub fn to_parsable_string(&self) -> String {
        let mut s = String::new();
        self.write_parsable_string(&mut s).unwrap();
        s
    }

    pub fn write_parsable_string<W: fmt::Write>(&self, f: &mut W) -> fmt::Result {
        write!(f, "{}-{}", self.start.as_u32(), self.end_inclusive.as_u32())
    }
}

impl Display for ShardGroup {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "ShardGroup(")?;
        self.write_parsable_string(f)?;
        write!(f, ")")
    }
}

impl FromStr for ShardGroup {
    type Err = ShardGroupParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('-');
        let start = parts.next().ok_or_else(|| ShardGroupParseError(s.to_string()))?;
        let start = start.parse::<u32>().map_err(|_| ShardGroupParseError(s.to_string()))?;
        let end = parts.next().ok_or_else(|| ShardGroupParseError(s.to_string()))?;
        let end = end.parse::<u32>().map_err(|_| ShardGroupParseError(s.to_string()))?;
        ShardGroup::new_checked(start, end).ok_or_else(|| ShardGroupParseError(s.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Invalid ShardGroup string '{0}'")]
pub struct ShardGroupParseError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode() {
        let sg = ShardGroup::new(123, 234);
        let n = sg.encode_as_u32();
        let sg2 = ShardGroup::decode_from_u32(n).unwrap();
        assert_eq!(sg, sg2);
        assert_eq!(ShardGroup::decode_from_u32(0), Some(ShardGroup::new(0, 0)));
        assert_eq!(
            ShardGroup::decode_from_u32(ShardGroup::MAX_ENCODED_VALUE),
            Some(ShardGroup::new(0x100, 0x100))
        );
        assert_eq!(ShardGroup::decode_from_u32(ShardGroup::MAX_ENCODED_VALUE + 1), None);
        assert_eq!(ShardGroup::decode_from_u32(u32::MAX), None);
    }

    #[test]
    fn to_substate_address_range() {
        let sg = ShardGroup::new(1, 64);
        let range = sg.to_substate_address_range(NumPreshards::P64);
        assert_eq!(*range.start(), SubstateAddress::zero());
        assert_eq!(*range.end(), SubstateAddress::max());
    }

    #[test]
    fn to_string_and_parsing() {
        let sg = ShardGroup::new(0, 63);
        let s = sg.to_parsable_string();
        assert_eq!(s, "0-63");
        let sg2 = s.parse::<ShardGroup>().unwrap();
        assert_eq!(sg, sg2);

        let n = u64::from(u32::MAX) + 1;
        format!("{n}-999").parse::<ShardGroup>().unwrap_err();

        "100-1".parse::<ShardGroup>().unwrap_err();
    }

    #[test]
    fn contains_or_global_works_correctly() {
        let sg = ShardGroup::new(10, 20);

        // Test with a global shard
        let global_shard = Shard::global(); // Assuming this constructor exists
        assert!(sg.contains_or_global(&global_shard));

        // Test with a contained shard
        let contained_shard = Shard::from(15);
        assert!(sg.contains_or_global(&contained_shard));

        // Test with a non-contained shard
        let non_contained_shard = Shard::from(30);
        assert!(!sg.contains_or_global(&non_contained_shard));
    }

    #[test]
    fn all_shards() {
        let sg = ShardGroup::all_shards(NumPreshards::P1);
        assert_eq!(sg, ShardGroup::new(1, 1));
        let sg = ShardGroup::all_shards(NumPreshards::P64);
        assert_eq!(sg, ShardGroup::new(1, 64));
    }
}
