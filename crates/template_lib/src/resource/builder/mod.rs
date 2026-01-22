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

pub mod confidential;
pub mod fungible;
pub mod non_fungible;
pub mod stealth;

pub use tari_template_lib_types::constants::{DEFAULT_DIVISIBILITY, IMAGE_URL, TOKEN_SYMBOL};

use crate::resource::{
    builder::{
        confidential::ConfidentialResourceBuilder,
        fungible::FungibleResourceBuilder,
        non_fungible::NonFungibleResourceBuilder,
    },
    stealth::StealthResourceBuilder,
};

/// Utility for building resources inside templates
pub struct ResourceBuilder {}

impl ResourceBuilder {
    /// Returns a new publicly visible fungible resource builder.
    ///
    /// WARNING: this resource is not confidential. Balances in vaults will be visible to anyone with access to the
    /// ledger.
    pub fn public_fungible() -> FungibleResourceBuilder {
        FungibleResourceBuilder::new()
    }

    /// Returns a new non-fungible resource builder.
    ///
    /// A vault containing this resource holds a collection of non-fungible tokens (NFTs), each with a unique
    /// identifier.
    ///
    /// WARNING: this resource is not confidential. NFTs in vaults will be visible to anyone with access to the
    /// ledger.
    pub fn non_fungible() -> NonFungibleResourceBuilder {
        NonFungibleResourceBuilder::new()
    }

    /// Returns a new confidential resource builder.
    ///
    /// Vaults containing this resource consist of blinded outputs in addition to a revealed portion that is publicly
    /// visible. A user may transfer funds to and from confidential outputs to the revealed balance. The primary use
    /// case of revealed balances is to allow excess funds, previously revealed to pay fees, to be refunded to the
    /// vault.
    pub fn confidential() -> ConfidentialResourceBuilder {
        ConfidentialResourceBuilder::new()
    }

    /// Returns a new stealth resource builder.
    ///
    /// The highest level of confidentiality. Funds are not kept in vaults, and each output is a confidential substate
    /// that lives on the ledger.
    pub fn stealth() -> StealthResourceBuilder {
        StealthResourceBuilder::new()
    }
}
