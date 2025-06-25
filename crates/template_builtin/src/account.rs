//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tari_bor::from_value;
use tari_template_lib::models::{ResourceAddress, Vault, VaultId};

/// Represents an account containing multiple vaults, each identified by a resource address.
/// This contains the same state as the `Account` built-in template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    vaults: BTreeMap<ResourceAddress, Vault>,
}

impl Account {
    pub fn from_value(value: &tari_bor::Value) -> Result<Self, tari_bor::BorError> {
        from_value(value)
    }

    pub fn vaults(&self) -> &BTreeMap<ResourceAddress, Vault> {
        &self.vaults
    }

    pub fn get_vault_by_resource(&self, resource_address: &ResourceAddress) -> Option<VaultId> {
        self.vaults.get(resource_address).map(|vault| vault.vault_id())
    }
}
