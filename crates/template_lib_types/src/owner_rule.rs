//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::{access_rules::AccessRule, crypto::RistrettoPublicKeyBytes};

/// An enum for all possible ways to specify ownership of values
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum OwnerRule {
    /// The owner is the signer of the transaction that created the value
    #[default]
    OwnedBySigner,
    /// There is no owner, only access rules apply
    None,
    /// The owner is anyone who satisfies an access rule
    ByAccessRule(AccessRule),
    /// The owner is a specific public key
    ByPublicKey(#[cfg_attr(feature = "ts", ts(type = "Array<number>"))] RistrettoPublicKeyBytes),
}

impl OwnerRule {
    pub fn owned_by_public_key(&self) -> Option<&RistrettoPublicKeyBytes> {
        match self {
            OwnerRule::ByPublicKey(key) => Some(key),
            _ => None,
        }
    }
}
