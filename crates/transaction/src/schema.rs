//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

/// The rules a transaction is signed, identified and verified under.
///
/// Every schema shares one body encoding: the schema is the variant of the transaction enum that wraps a body,
/// and the first value each signing and id digest chains, so a signature made under one schema never verifies
/// under another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemaVersion {
    /// Authorizations sign the body and the seal signer's key. The id hashes the authorization signatures as
    /// listed, in any order.
    V1,
    /// Authorizations also sign the set of every authorizer's key, the list must be strictly ascending by key,
    /// and the id hashes the authorizers' keys rather than their signatures.
    V2,
}

impl SchemaVersion {
    /// The schema every transaction is built under.
    pub const CURRENT: Self = Self::V2;

    pub const fn as_u16(self) -> u16 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
        }
    }

    /// True when authorizations sign the authorizer set and the authorization list must be canonical.
    pub const fn binds_authorizer_set(self) -> bool {
        matches!(self, Self::V2)
    }
}
