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

//! This module provides the `StealthResourceManager` struct, a high-level interface for managing
//! Tari Ootle resources. Resources can be non-private fungible tokens, non-fungible tokens, confidential fungible and
//! stealth fungible.
//!
//! It abstracts common operations like creating resources, minting tokens, recalling tokens from vaults,
//! querying supply, and updating non-fungible metadata.
//!
//! The `StealthResourceManager` uses engine calls to perform resource operations and enforces
//! access rules and permissions based on the resource configuration.
//!
//! # Examples
//!
//! ```rust,ignore
//! use tari_template_lib::resource::manager::StealthResourceManager;
//! let resource_manager = StealthResourceManager::get(my_resource_address);
//! resource_manager.mint_fungible(1000);
//! ```

use serde::{Deserialize, Serialize};
use tari_template_abi::{call_engine, EngineOp};

use crate::{
    args::{
        FreezeResourceArg,
        InvokeResult,
        MintArg,
        MintResourceArg,
        ResourceAction,
        ResourceInvokeArg,
        VaultFreezeFlags,
    },
    auth::ResourceAccessRules,
    models::{Bucket, BucketId, ResourceAddress, StealthMintStatement, VaultId},
    types::Amount,
};

/// Provides an interface for various resource operations e.g. Minting, recalling, creating, etc.
///
/// # Example
/// ```rust,ignore
/// use tari_template_lib::resource::manager::StealthResourceManager;
/// let resource_manager = StealthResourceManager::get(my_resource_address);
/// resource_manager.mint_fungible(Amount(1000));
/// ```
#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StealthResourceManager {
    resource_address: ResourceAddress,
}

impl StealthResourceManager {
    /// Returns the address of the resource that is being managed
    pub fn get(resource_address: ResourceAddress) -> Self {
        Self { resource_address }
    }

    /// Returns the address of the resource that is being managed.
    pub fn resource_address(&self) -> ResourceAddress {
        self.resource_address
    }

    /// Mints new tokens for the stealth resource managed by this `StealthResourceManager`.
    ///
    /// This method accepts a zero-knowledge proof that authorizes the minting of stealth tokens.
    ///
    /// # Arguments
    ///
    /// * `statement` – A [`ConfidentialOutputStatement`] containing the outputs to mint. This the outputs to mint, and
    ///   a range proof.
    ///
    /// # Panics
    ///
    /// This method will panic if:
    /// - The resource is not of type [`ResourceType::Confidential`]
    /// - The provided statement is invalid or malformed
    /// - The caller lacks the required minting permissions, as defined by the resource's [`ResourceAccessRules`]
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let bucket = resource_manager.mint_confidential(statement);
    /// ```
    pub fn mint(&self, statement: StealthMintStatement) {
        self.mint_internal(MintResourceArg {
            mint_arg: MintArg::Stealth {
                statement: Box::new(statement),
            },
        });
    }

    /// Returns the total supply of tokens for the resource being managed in a [`StealthResourceManager`] instance.
    ///
    /// If the resource has total supply tracking enabled, the function will return the total supply of tokens.
    ///
    /// If you want to check if the resource has total supply tracking enabled, use [`total_supply_opt`].
    ///
    /// # Panics
    ///
    /// Panics if:
    /// * the resource does not have total supply tracking enabled. You can check this with [`total_supply_opt`].
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let total = resource_manager.total_supply();
    /// println!("Total supply: {}", total);
    /// ```    
    pub fn total_supply(&self) -> Amount {
        self.total_supply_opt()
            .expect("Resource does not have total supply tracking enabled")
    }

    /// Returns the total supply of the resource as an `Option<Amount>`.
    ///
    /// This method invokes the resource engine with a `GetTotalSupply` action
    /// to query the current total supply tracked by the resource. If the resource
    /// does not have total supply tracking enabled, this will return `None`.
    ///
    /// # Returns
    ///
    /// * `Some(Amount)` if total supply tracking is enabled and available.
    /// * `None` if total supply tracking is not enabled.
    ///
    /// # Panics
    ///
    /// Panics if decoding the response into an `Option<Amount>` fails.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let total_supply = resource_manager.total_supply_opt();
    /// if let Some(amount) = total_supply {
    ///     println!("Total supply is {:?}", amount);
    /// } else {
    ///     println!("Total supply tracking not enabled");
    /// }
    /// ```
    pub fn total_supply_opt(&self) -> Option<Amount> {
        let resp: InvokeResult = call_engine(EngineOp::ResourceInvoke, &ResourceInvokeArg {
            resource_ref: self.resource_address.into(),
            action: ResourceAction::GetTotalSupply,
            args: invoke_args![],
        });

        resp.decode().expect("[total_supply] Failed to decode Amount")
    }

    /// Updates access rules that determine who can operate the resource
    ///
    /// The function allows the caller to overwrite the existing [`ResourceAccessRules`] for the resource with a new
    /// set. This will replace the existing access rules entirely.
    ///
    /// # Arguments
    ///
    /// * `access_rules` - The new [`ResourceAccessRules`] to set for the resource.
    ///
    /// # Panics
    ///
    /// It will panic if:
    /// - The caller does not have the necessary [`ResourceAccessRules`] or [`OwnerRule`] to update the access rules.
    /// - The [`ResourceAccessRules`] are invalid or malformed.
    ///
    /// # Examples
    /// ```rust,ignore
    /// let new_access_rules = ResourceAccessRules::default()
    /// resource_manager.set_access_rules(new_access_rules);
    /// ```
    pub fn set_access_rules(&self, access_rules: ResourceAccessRules) {
        let resp: InvokeResult = call_engine(EngineOp::ResourceInvoke, &ResourceInvokeArg {
            resource_ref: self.resource_address.into(),
            action: ResourceAction::UpdateAccessRules,
            args: invoke_args![access_rules],
        });

        resp.decode().expect("[set_access_rules] Failed")
    }

    /// Freezes all withdrawals, deposits and burns for the specified vault.
    pub fn freeze(&self, vault_id: VaultId) {
        self.set_freeze(vault_id, VaultFreezeFlags::all());
    }

    /// Sets the freeze flags for the specified vault.
    pub fn set_freeze(&self, vault_id: VaultId, flags: VaultFreezeFlags) {
        let resp: InvokeResult = call_engine(EngineOp::ResourceInvoke, &ResourceInvokeArg {
            resource_ref: self.resource_address.into(),
            action: ResourceAction::SetFreeze,
            args: invoke_args![FreezeResourceArg { vault_id, flags }],
        });

        resp.decode().expect("SetFreeze failed")
    }

    /// Unfreezes all withdrawals, deposits and burns for the specified vault.
    /// Equivalent to `manager.set_freeze(FreezeFlags::empty())`.
    pub fn unfreeze(&self, vault_id: VaultId) {
        self.set_freeze(vault_id, VaultFreezeFlags::empty());
    }

    fn mint_internal(&self, arg: MintResourceArg) -> Option<Bucket> {
        let resp: InvokeResult = call_engine(EngineOp::ResourceInvoke, &ResourceInvokeArg {
            resource_ref: self.resource_address.into(),
            action: ResourceAction::Mint,
            args: invoke_args![arg],
        });

        let maybe_bucket_id: Option<BucketId> = resp.decode().expect("Failed to decode Bucket");
        maybe_bucket_id.map(Bucket::from_id)
    }
}

impl From<ResourceAddress> for StealthResourceManager {
    fn from(resource_address: ResourceAddress) -> Self {
        Self::get(resource_address)
    }
}
