//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::{fmt, prelude::*};

/// Represents every possible type of resource in the Tari network.
///
/// Resources represent digital assets managed within the Tari system, including
/// fungible tokens, non-fungible tokens (NFTs), and confidential fungible tokens.
///
/// - **Fungible** tokens are interchangeable and divisible (e.g., currency, shares).
/// - **NonFungible** tokens represent unique, indivisible assets (e.g., collectibles).
/// - **Confidential** A type of fungible resource that uses cryptographic privacy to keep balances confidential. Funds
///   are placed in vaults and can therefore be associated with a component that contains them, typically an Account.
/// - **Stealth** A fungible resource using the highest level of confidentiality. Funds are not kept in vaults, and each
///   output is an independent confidential substate (kind of like creating a new unlinked vault for each currency
///   note).
#[derive(Clone, Copy, Debug, PartialEq, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub enum ResourceType {
    /// Fungible tokens do not have individual identity, making them interchangeable.
    /// Examples include monetary units, liquidity pool tokens, or tokenized shares.
    #[n(0)]
    Fungible,
    /// A resource (i.e., collection) of non-fungible tokens.
    /// Each NFT is uniquely identifiable within the parent resource and indivisible.
    #[n(1)]
    NonFungible,
    /// A type of fungible resource that uses cryptographic privacy to keep balances confidential. Funds are placed in
    /// vaults and can therefore be associated with a component that contains them, typically an Account.
    #[n(2)]
    Confidential,
    /// A fungible resource using the highest level of confidentiality. Funds are not kept in vaults, and each output
    /// is an independent confidential substate (kind of like creating a new unlinked vault for each currency note).
    #[n(3)]
    Stealth,
}

impl ResourceType {
    /// Returns `true` if the resource type is fungible, otherwise `false`.
    pub const fn is_public_fungible(&self) -> bool {
        matches!(self, Self::Fungible)
    }

    /// Returns `true` if the resource type is non-fungible, otherwise `false`.
    pub const fn is_non_fungible(&self) -> bool {
        matches!(self, Self::NonFungible)
    }

    /// Returns `true` if the resource type is confidential fungible, otherwise `false`.
    pub const fn is_confidential(&self) -> bool {
        matches!(self, Self::Confidential)
    }

    /// Returns `true` if the resource type is stealth, otherwise `false`.
    pub const fn is_stealth(&self) -> bool {
        matches!(self, Self::Stealth)
    }
}

impl fmt::Display for ResourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[cfg(feature = "std")]
mod parsing {
    use std::str::FromStr;

    use super::*;

    impl FromStr for ResourceType {
        type Err = ParseResourceTypeError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "Fungible" => Ok(ResourceType::Fungible),
                "NonFungible" | "nft" => Ok(ResourceType::NonFungible),
                "Confidential" => Ok(ResourceType::Confidential),
                "Stealth" => Ok(ResourceType::Stealth),
                _ => Err(ParseResourceTypeError(s.to_string())),
            }
        }
    }

    #[derive(Debug)]
    pub struct ParseResourceTypeError(String);

    impl fmt::Display for ParseResourceTypeError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "Invalid Resource Type string: '{}'", self.0)
        }
    }

    impl std::error::Error for ParseResourceTypeError {}
}
#[cfg(feature = "std")]
pub use parsing::ParseResourceTypeError;

/// Info for a resource, including its type and divisibility.
#[derive(Clone, Copy, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ResourceInfo {
    #[n(0)]
    pub resource_type: ResourceType,
    #[n(1)]
    pub divisibility: u8,
}
