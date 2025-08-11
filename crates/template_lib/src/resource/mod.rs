//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! Utilities for building and managing resources inside templates.
//!
//! This module provides abstractions to define and work with various resource types in the Tari network.
//! Resources can be fungible tokens, non-fungible tokens, or confidential fungible tokens with privacy features.
//!
//! The `ResourceType` enum categorizes these resource types and offers convenience methods for checking the type.
//!
//! # Example
//! ```rust
//! use tari_template_lib::resource::ResourceType;
//!
//! let resource = ResourceType::Fungible;
//! assert!(resource.is_fungible());
//! ```
//!
//! The module also re-exports builders and managers for resource creation and lifecycle management.

use tari_template_abi::rust::{fmt, str::FromStr};

mod builder;
mod manager;

pub use builder::*;
pub use manager::*;

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
///
/// This enum is serializable/deserializable with `serde` and optionally generates
/// TypeScript bindings when the `ts` feature is enabled.

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub enum ResourceType {
    /// Fungible tokens do not have individual identity, making them interchangeable.
    /// Examples include monetary units, liquidity pool tokens, or tokenized shares.
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
    pub fn is_fungible(&self) -> bool {
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

impl std::error::Error for ParseResourceTypeError {}
