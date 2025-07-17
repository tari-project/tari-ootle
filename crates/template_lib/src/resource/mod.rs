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
//! use your_crate::resource::ResourceType;
//!
//! let resource = ResourceType::Fungible;
//! assert!(resource.is_fungible());
//! ```
//!
//! The module also re-exports builders and managers for resource creation and lifecycle management.

mod builder;

use std::fmt::Display;

pub use builder::*;
mod manager;
pub use manager::*;
#[cfg(feature = "ts")]
use ts_rs::TS;

/// Represents every possible type of resource in the Tari network.
///
/// Resources represent digital assets managed within the Tari system, including
/// fungible tokens, non-fungible tokens (NFTs), and confidential fungible tokens.
///
/// - **Fungible** tokens are interchangeable and divisible (e.g., currency, shares).
/// - **NonFungible** tokens represent unique, indivisible assets (e.g., collectibles).
/// - **Confidential** tokens are fungible tokens with privacy-preserving features.
///
/// This enum is serializable/deserializable with `serde` and optionally generates
/// TypeScript bindings when the `ts` feature is enabled.

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, export_to = "../../bindings/src/types/"))]
pub enum ResourceType {
    /// Fungible tokens do not have individual identity, making them interchangeable.
    /// Examples include monetary units, liquidity pool tokens, or tokenized shares.
    Fungible,
    /// A resource (i.e., collection) of non-fungible tokens.
    /// Each NFT is uniquely identifiable within the parent resource and indivisible.
    NonFungible,
    /// A type of fungible resource that uses cryptographic privacy to keep balances confidential.
    Confidential,
}

impl ResourceType {
    /// Returns `true` if the resource type is fungible.
    pub fn is_fungible(&self) -> bool {
        matches!(self, Self::Fungible)
    }

    /// Returns `true` if the resource type is non-fungible.
    pub fn is_non_fungible(&self) -> bool {
        matches!(self, Self::NonFungible)
    }

    /// Returns `true` if the resource type is confidential fungible.
    pub fn is_confidential(&self) -> bool {
        matches!(self, Self::Confidential)
    }
}

impl Display for ResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
