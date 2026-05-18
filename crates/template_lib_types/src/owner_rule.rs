//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{Decode, Encode};

use crate::{access_rules::AccessRule, crypto::RistrettoPublicKeyBytes};

/// An enum for all possible ways to specify ownership of values
#[derive(Debug, Clone, Default, Encode, Decode, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum OwnerRule {
    /// The owner is the signer of the transaction that created the value
    #[default]
    #[n(0)]
    OwnedBySigner,
    /// There is no owner, only access rules apply
    #[n(1)]
    None,
    /// The owner is anyone who satisfies an access rule
    #[n(2)]
    ByAccessRule(#[n(0)] AccessRule),
    /// The owner is a specific public key
    #[n(3)]
    ByPublicKey(
        #[n(0)]
        #[cfg_attr(feature = "ts", ts(type = "Array<number>"))]
        RistrettoPublicKeyBytes,
    ),
}

impl OwnerRule {
    pub fn owned_by_public_key(&self) -> Option<&RistrettoPublicKeyBytes> {
        match self {
            Self::ByPublicKey(key) => Some(key),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum SubstateOwnerRule {
    /// There is no owner, only access rules apply
    #[n(0)]
    None,
    /// The owner is anyone who satisfies an access rule
    #[n(1)]
    ByAccessRule(#[n(0)] AccessRule),
    /// The owner is a specific public key
    #[n(2)]
    ByPublicKey(
        #[n(0)]
        #[cfg_attr(feature = "ts", ts(type = "Array<number>"))]
        RistrettoPublicKeyBytes,
    ),
}

impl SubstateOwnerRule {
    pub fn owned_by_public_key(&self) -> Option<&RistrettoPublicKeyBytes> {
        match self {
            Self::ByPublicKey(key) => Some(key),
            _ => None,
        }
    }
}
