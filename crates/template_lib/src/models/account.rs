//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_bor::from_value;
use tari_template_abi::rust::collections::BTreeMap;
use tari_template_lib_types::ResourceAddress;

use crate::models::Vault;

/// Represents an account containing multiple vaults, each identified by a resource address.
/// Accounts can be decoded in templates by using as follows:
/// ```ignore,rust
/// use tari_template_lib::types::Account;
/// let component_state = cbor!(); // .. get state e.g. by using caller.component_state() in a auth hook
/// let account = Account::from_value(&component_state)?;
/// ```
#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Account {
    #[n(0)]
    vaults: BTreeMap<ResourceAddress, Vault>,
}

impl Account {
    /// Decodes state into an `Account`
    pub fn from_value(value: &tari_bor::Value) -> Result<Self, tari_bor::BorError> {
        from_value(value)
    }

    /// Returns the vault map
    pub fn vaults(&self) -> &BTreeMap<ResourceAddress, Vault> {
        &self.vaults
    }

    /// Finds a vault by its resource address.
    pub fn get_vault_by_resource(&self, resource_address: &ResourceAddress) -> Option<&Vault> {
        self.vaults.get(resource_address)
    }

    /// Returns an iterator over all resource addresses in the account.
    pub fn all_resources_iter(&self) -> impl Iterator<Item = &ResourceAddress> {
        self.vaults.keys()
    }
}
