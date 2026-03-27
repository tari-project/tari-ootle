//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_engine::state_store::{StateReader, StateStoreError, memory::MemoryStateStore};
use tari_engine_types::{
    Utxo,
    component::ComponentHeader,
    indexed_value::IndexedValue,
    resource::Resource,
    substate::{Substate, SubstateId},
    vault::Vault,
};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{
    models::Account,
    prelude::RistrettoPublicKeyBytes,
    types::{ComponentAddress, ResourceAddress, TemplateAddress, UtxoAddress, VaultId},
};

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

    pub fn get_components_by_template_address(
        &self,
        template_address: TemplateAddress,
    ) -> Result<Vec<(ComponentAddress, ComponentHeader)>, StateStoreError> {
        let mut components = Vec::new();
        self.with_substates(|id, substate| {
            if let SubstateId::Component(component_address) = id &&
                let Some(component) = substate.substate_value().as_component() &&
                component.template_address == template_address
            {
                components.push((*component_address, component.clone()));
            }
        })?;
        Ok(components)
    }

    pub fn get_account(&self, account_address: ComponentAddress) -> Result<Account, StateStoreError> {
        let account = self.get_component(account_address)?;
        Account::from_value(account.state()).map_err(|e| StateStoreError::CustomStr(e.to_string()))
    }

    pub fn all_accounts(&self) -> Result<HashMap<ComponentAddress, Account>, StateStoreError> {
        let mut accounts = HashMap::new();
        self.with_substates(|id, substate| {
            if let SubstateId::Component(component_address) = id &&
                let Some(component) = substate.substate_value().as_component() &&
                component.template_address == ACCOUNT_TEMPLATE_ADDRESS &&
                let Ok(account) = Account::from_value(component.state())
            {
                accounts.insert(*component_address, account);
            }
        })?;
        Ok(accounts)
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

    pub fn get_all_resources(&self) -> Result<HashMap<ResourceAddress, Resource>, StateStoreError> {
        let mut resources = HashMap::new();
        self.with_substates(|id, substate| {
            if let SubstateId::Resource(resource_address) = id {
                let resource = substate.substate_value().as_resource().unwrap();
                resources.insert(*resource_address, resource.clone());
            }
        })?;
        Ok(resources)
    }

    pub fn get_resources_by_owner(
        &self,
        owner: &RistrettoPublicKeyBytes,
    ) -> Result<Vec<(ResourceAddress, Resource)>, StateStoreError> {
        let mut resources = Vec::new();
        self.with_substates(|id, substate| {
            if let SubstateId::Resource(resource_address) = id &&
                let Some(resource) = substate.substate_value().as_resource() &&
                resource.owner_rule().owned_by_public_key() == Some(owner)
            {
                resources.push((*resource_address, resource.clone()));
            }
        })?;
        Ok(resources)
    }

    pub fn get_vault(&self, vault_id: &VaultId) -> Result<Vault, StateStoreError> {
        let substate = self.get_substate(&SubstateId::Vault(*vault_id))?;
        Ok(substate.into_substate_value().into_vault().unwrap())
    }

    pub fn get_utxo(&self, utxo_addr: UtxoAddress) -> Result<Utxo, StateStoreError> {
        let substate = self.get_substate(&SubstateId::Utxo(utxo_addr))?;
        Ok(substate.into_substate_value().into_utxo().unwrap())
    }

    pub fn get_all_utxos(&self) -> Result<Vec<(UtxoAddress, Utxo)>, StateStoreError> {
        let mut utxos = Vec::new();
        self.with_substates(|id, substate| {
            if let SubstateId::Utxo(utxo_addr) = id {
                let utxo = substate.substate_value().as_utxo().unwrap();
                utxos.push((utxo_addr.clone(), utxo.clone()));
            }
        })?;
        Ok(utxos)
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
    where F: FnMut(&SubstateId, &Substate) {
        self.store.iter().for_each(|(id, substate)| f(id, substate));
        Ok(())
    }
}
