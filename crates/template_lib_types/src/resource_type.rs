//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::{fmt, str::FromStr};

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
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub enum ResourceType {
    /// Fungible tokens do not have individual identity, making them interchangeable.
    /// Examples include monetary units, liquidity pool tokens, or tokenized shares.
    // TODO: rename to PublicFungible
    Fungible,
    /// A resource (i.e., collection) of non-fungible tokens.
    /// Each NFT is uniquely identifiable within the parent resource and indivisible.
    NonFungible,
    /// A type of fungible resource that uses cryptographic privacy to keep balances confidential. Funds are placed in
    /// vaults and can therefore be associated with a component that contains them, typically an Account.
    Confidential,
    /// A fungible resource using the highest level of confidentiality. Funds are not kept in vaults, and each output
    /// is an independent confidential substate (kind of like creating a new unlinked vault for each currency note).
    Stealth,
}

impl ResourceType {
    /// Returns `true` if the resource type is fungible, otherwise `false`.
    pub fn is_public_fungible(&self) -> bool {
        matches!(self, Self::Fungible)
    }

    /// Returns `true` if the resource type is non-fungible, otherwise `false`.
    pub fn is_non_fungible(&self) -> bool {
        matches!(self, Self::NonFungible)
    }

    /// Returns `true` if the resource type is confidential fungible, otherwise `false`.
    pub fn is_confidential(&self) -> bool {
        matches!(self, Self::Confidential)
    }

    /// Returns `true` if the resource type is stealth, otherwise `false`.
    pub fn is_stealth(&self) -> bool {
        matches!(self, Self::Stealth)
    }
}

impl fmt::Display for ResourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

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

#[cfg(feature = "std")]
impl std::error::Error for ParseResourceTypeError {}
