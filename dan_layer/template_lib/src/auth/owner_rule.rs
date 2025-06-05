//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::borrow::Cow;

use crate::{auth::AccessRule, types::crypto::RistrettoPublicKeyBytes};

/// Data that is needed to represent ownership of a value (resource or component method).
/// Owners are the only ones allowed to update the values's access rules after creation
#[derive(Debug, Clone)]
pub struct Ownership<'a> {
    pub owner_key: Option<&'a RistrettoPublicKeyBytes>,
    pub owner_rule: Cow<'a, OwnerRule>,
}

/// An enum for all possible ways to specify ownership of values
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
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
