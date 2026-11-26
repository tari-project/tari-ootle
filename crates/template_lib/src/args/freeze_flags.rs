//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::{fmt, fmt::Display, iter, ops};

const ALL_FLAGS: u8 = VaultFreezeFlag::Deposits as u8 | VaultFreezeFlag::Withdrawals as u8;

#[derive(Clone, Debug, Copy, Default, Encode, Decode, CborLen)]
#[cbor(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct VaultFreezeFlags(#[n(0)] u8);

impl VaultFreezeFlags {
    pub fn empty() -> Self {
        Self(0)
    }

    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    pub const fn contains(&self, flag: VaultFreezeFlag) -> bool {
        (self.0 & flag as u8) != 0
    }

    pub const fn raw(&self) -> u8 {
        self.0
    }

    pub const fn all() -> Self {
        Self(ALL_FLAGS)
    }

    #[must_use]
    pub const fn validate(self) -> bool {
        self.0 & Self::all().0 == self.0
    }

    pub fn iter(&self) -> impl Iterator<Item = VaultFreezeFlag> + '_ {
        let mut mask = 1u8;

        iter::from_fn(move || {
            while mask <= ALL_FLAGS {
                let current = mask;
                mask <<= 1; // Move to the next bit
                if (self.0 & current) != 0 {
                    return Some(VaultFreezeFlag::from_byte(current).expect("mask is always a valid flag"));
                }
            }
            None
        })
    }
}

impl From<VaultFreezeFlag> for VaultFreezeFlags {
    fn from(flag: VaultFreezeFlag) -> Self {
        Self(flag as u8)
    }
}

impl Display for VaultFreezeFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return write!(f, "<none>");
        }
        let count = self.iter().count();

        for (i, flag) in self.iter().enumerate() {
            write!(f, "{}", flag)?;
            if i < count - 1 {
                write!(f, ",")?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum VaultFreezeFlag {
    #[n(0)]
    Deposits = 1 << 0,
    #[n(1)]
    Withdrawals = 1 << 1,
}

impl VaultFreezeFlag {
    pub fn from_byte(b: u8) -> Option<Self> {
        if b == Self::Deposits as u8 {
            return Some(Self::Deposits);
        }
        if b == Self::Withdrawals as u8 {
            return Some(Self::Withdrawals);
        }

        None // Invalid flag
    }
}

impl ops::BitOr for VaultFreezeFlag {
    type Output = VaultFreezeFlags;

    fn bitor(self, rhs: Self) -> Self::Output {
        VaultFreezeFlags(self as u8 | rhs as u8)
    }
}

impl Display for VaultFreezeFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Deposits => write!(f, "Deposits"),
            Self::Withdrawals => write!(f, "Withdrawals"),
        }
    }
}

#[cfg(test)]
mod tests {
    use tari_template_abi::rust::string::ToString;

    use super::*;

    #[test]
    fn it_unions_bits() {
        let flag1 = VaultFreezeFlag::Deposits;
        let flag2 = VaultFreezeFlag::Withdrawals;
        let combined = flag1 | flag2;

        assert_eq!(combined.raw(), 0b0011); // 0b0001 | 0b0010 = 0b0011

        assert!(combined.contains(flag1));
        assert!(combined.contains(flag2));
    }

    #[test]
    fn it_iterates() {
        let flags = VaultFreezeFlags::all();
        let mut iter = flags.iter();

        assert!(matches!(iter.next(), Some(VaultFreezeFlag::Deposits)));
        assert!(matches!(iter.next(), Some(VaultFreezeFlag::Withdrawals)));
        assert!(iter.next().is_none()); // No more flags
    }

    #[test]
    fn it_iterates_and_formats_when_first_flag_is_not_set() {
        let withdrawals_only = VaultFreezeFlags::from(VaultFreezeFlag::Withdrawals);
        let collected: Vec<_> = withdrawals_only.iter().collect();
        assert_eq!(collected, vec![VaultFreezeFlag::Withdrawals]);
        assert_eq!(withdrawals_only.iter().count(), 1);
        assert_eq!(withdrawals_only.to_string(), "Withdrawals");
    }

    #[test]
    fn it_validates_if_all_flags_are_known() {
        let flags = VaultFreezeFlags::all();
        assert!(flags.validate());

        let invalid_flags = VaultFreezeFlags(0b1000); // Invalid flag
        assert!(!invalid_flags.validate());
    }
}
