//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_engine::state_store::{memory::MemoryStateStore, StateReader, StateStoreError};
use tari_engine_types::{
    component::ComponentHeader,
    indexed_value::IndexedValue,
    resource::Resource,
    substate::{Substate, SubstateId},
    vault::Vault,
};
use tari_template_lib::models::{Account, ComponentAddress, ResourceAddress, VaultId};

pub struct ReadOnlyStateStore<'a> {
    store: &'a MemoryStateStore,
}
impl<'a> ReadOnlyStateStore<'a> {
    pub fn new(store: &'a MemoryStateStore) -> Self {
        Self { store }
    }

    pub fn get_component(&self, component_address: ComponentAddress) -> Result<ComponentHeader, StateStoreError> {
        let substate = self.get_substate(&SubstateId::Component(component_address))?;
        Ok(substate.into_substate_value().into_component().unwrap())
    }

    pub fn get_account(&self, account_address: ComponentAddress) -> Result<Account, StateStoreError> {
        let account = self.get_component(account_address)?;
        Account::from_value(account.state()).map_err(|e| StateStoreError::CustomStr(e.to_string()))
    }

    pub fn get_vaults_for_account(
        &self,
        account_address: ComponentAddress,
    ) -> Result<HashMap<ResourceAddress, Vault>, StateStoreError> {
        let account = self.get_account(account_address)?;
        let mut vaults = HashMap::with_capacity(account.vaults().len());
        for (resource_addr, vault) in account.vaults() {
            let vault_id = vault.vault_id();
            let vault = self.get_vault(&vault_id)?;
            vaults.insert(*resource_addr, vault);
        }
        Ok(vaults)
    }

    pub fn get_resource(&self, resource_address: &ResourceAddress) -> Result<Resource, StateStoreError> {
        let substate = self.get_substate(&SubstateId::Resource(*resource_address))?;
        Ok(substate.into_substate_value().into_resource().unwrap())
    }

    pub fn get_vault(&self, vault_id: &VaultId) -> Result<Vault, StateStoreError> {
        let substate = self.get_substate(&SubstateId::Vault(*vault_id))?;
        Ok(substate.into_substate_value().into_vault().unwrap())
    }

    pub fn inspect_component(&self, component_address: ComponentAddress) -> Result<IndexedValue, StateStoreError> {
        let component = self.get_component(component_address)?;
        Ok(IndexedValue::from_value(component.into_state()).unwrap())
    }

    pub fn count(&self) -> Result<usize, StateStoreError> {
        let count = self.store.count();
        Ok(count)
    }

    pub fn get_substate(&self, id: &SubstateId) -> Result<Substate, StateStoreError> {
        let substate = self.store.get_state(id)?;
        Ok(substate.clone())
    }

    pub fn with_substates<F>(&self, mut f: F) -> Result<(), StateStoreError>
    where F: FnMut(Substate) {
        self.store.iter().for_each(|(_, substate)| f(substate.clone()));
        Ok(())
    }
}
