//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use tari_engine_types::{
    component::derive_component_address_from_public_key,
    indexed_value::IndexedWellKnownTypes,
    ToByteType,
};
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    substate_type::SubstateType,
};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{
    models::{ComponentAddress, ResourceAddress, VaultId},
    prelude::{ResourceType, RistrettoPublicKeyBytes},
    types::Amount,
};

use crate::{
    apis::{
        confidential_transfer::{ConfidentialTransferApiError, ResolvedAccountDetails},
        key_manager::{KeyBranch, KeyManagerApi, KeyManagerApiError},
        substate::{SubstatesApi, ValidatorScanResult},
    },
    models::{Account, AccountUpdate, AccountWithAddress, VaultBalance, VaultModel},
    network::WalletNetworkInterface,
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

pub struct AccountsApi<'a, TStore, TNetworkInterface> {
    store: &'a TStore,
    substates_api: SubstatesApi<'a, TStore, TNetworkInterface>,
    key_manager_api: KeyManagerApi<'a, TStore>,
}

pub fn derive_account_address_from_public_key(public_key: &RistrettoPublicKeyBytes) -> ComponentAddress {
    derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, public_key)
}

impl<'a, TStore: WalletStore, TNetworkInterface> AccountsApi<'a, TStore, TNetworkInterface> {
    pub fn new(
        store: &'a TStore,
        substates_api: SubstatesApi<'a, TStore, TNetworkInterface>,
        key_manager_api: KeyManagerApi<'a, TStore>,
    ) -> Self {
        Self {
            store,
            substates_api,
            key_manager_api,
        }
    }

    pub fn derive_account_address_from_public_key(&self, public_key: &RistrettoPublicKeyBytes) -> ComponentAddress {
        derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, public_key)
    }

    pub fn create_account(
        &self,
        account_name: Option<&str>,
        is_default: bool,
        key_id: Option<u64>,
    ) -> Result<AccountWithAddress, AccountsApiError> {
        let owner_address = match key_id {
            Some(id) => self.key_manager_api.derive_account_address(id)?,
            None => self.key_manager_api.next_account_address()?,
        };
        let owner_public_key = owner_address.address.account_key().to_byte_type();
        let account_component_address = derive_account_address_from_public_key(&owner_public_key);

        self.store.with_write_tx(|tx| {
            if let Some(name) = account_name {
                if tx.accounts_get_by_name(name).optional()?.is_some() {
                    return Err(AccountsApiError::AccountNameAlreadyExists { name: name.to_string() });
                }
            }

            tx.accounts_insert(
                account_name,
                &account_component_address,
                owner_address.key_index,
                false,
                is_default,
            )?;
            Ok(AccountWithAddress {
                account: Account {
                    name: account_name.map(String::from),
                    component_address: account_component_address,
                    key_index: owner_address.key_index,
                    is_confirmed_on_chain: false,
                    is_default,
                },
                address: owner_address.address.to_byte_type(),
            })
        })
    }

    pub fn add_account(
        &self,
        account_name: Option<&str>,
        account_address: &ComponentAddress,
        owner_key_index: u64,
        is_confirmed_on_chain: bool,
        is_default: bool,
    ) -> Result<(), AccountsApiError> {
        self.store.with_write_tx(|tx| {
            if let Some(name) = account_name {
                if tx.accounts_get_by_name(name).optional()?.is_some() {
                    return Err(AccountsApiError::AccountNameAlreadyExists { name: name.to_string() });
                }
            }
            tx.key_manager_insert_or_ignore(KeyBranch::Account.as_str(), owner_key_index)?;
            tx.accounts_insert(
                account_name,
                account_address,
                owner_key_index,
                is_confirmed_on_chain,
                is_default,
            )?;
            Ok(())
        })
    }

    pub fn update_account(
        &self,
        account_address: &ComponentAddress,
        update: AccountUpdate<'_>,
    ) -> Result<(), AccountsApiError> {
        self.store.with_write_tx(|tx| {
            tx.accounts_update(account_address, update)?;
            Ok(())
        })
    }

    pub fn associate_stealth_resource(
        &self,
        account_address: &ComponentAddress,
        stealth_resource_address: ResourceAddress,
    ) -> Result<(), AccountsApiError> {
        self.store
            .with_write_tx(|tx| tx.accounts_add_stealth_resource(account_address, stealth_resource_address))?;
        Ok(())
    }

    pub fn get_many(&self, offset: u64, limit: u64) -> Result<Vec<Account>, AccountsApiError> {
        let mut tx = self.store.create_read_tx()?;
        let accounts = tx.accounts_get_many(offset, limit)?;
        Ok(accounts)
    }

    pub fn count(&self) -> Result<u64, AccountsApiError> {
        let mut tx = self.store.create_read_tx()?;
        let count = tx.accounts_count()?;
        Ok(count)
    }

    pub fn any_accounts_exist(&self) -> Result<bool, AccountsApiError> {
        self.count().map(|count| count > 0)
    }

    pub fn get_default(&self) -> Result<AccountWithAddress, AccountsApiError> {
        // TODO: be careful not to use the key manager with a read transaction open as this will deadlock. The DB
        // transaction should be passed into the SDK methods to avoid this.
        let account = self.store.with_read_tx(|tx| tx.accounts_get_default())?;
        let address = self.key_manager_api.derive_account_address(account.key_index)?;
        Ok(AccountWithAddress {
            account,
            address: address.address.to_byte_type(),
        })
    }

    pub fn get_account_by_name(&self, name: &str) -> Result<AccountWithAddress, AccountsApiError> {
        let account = self.store.with_read_tx(|tx| tx.accounts_get_by_name(name))?;
        let address = self.key_manager_api.derive_account_address(account.key_index)?;
        Ok(AccountWithAddress {
            account,
            address: address.address.to_byte_type(),
        })
    }

    pub fn update_vault_balance(
        &self,
        vault_address: VaultId,
        revealed_balance: Amount,
        confidential_balance: Amount,
    ) -> Result<(), AccountsApiError> {
        self.store
            .with_write_tx(|tx| tx.vaults_update(vault_address, revealed_balance, confidential_balance))?;
        Ok(())
    }

    pub fn get_vault_balance(&self, vault_address: &VaultId) -> Result<VaultBalance, AccountsApiError> {
        let vault = self.store.with_read_tx(|tx| tx.vaults_get(vault_address))?;
        Ok(VaultBalance {
            account: vault.account_address,
            confidential: vault.confidential_balance,
            revealed: vault.revealed_balance,
        })
    }

    pub fn exists_by_address(&self, address: &ComponentAddress) -> Result<bool, AccountsApiError> {
        Ok(self.get_account_by_address(address).optional()?.is_some())
    }

    pub fn get_account_by_address(&self, address: &ComponentAddress) -> Result<AccountWithAddress, AccountsApiError> {
        let account = self.store.with_read_tx(|tx| tx.accounts_get(address))?;
        let address = self.key_manager_api.derive_account_address(account.key_index)?;
        Ok(AccountWithAddress {
            account,
            address: address.address.to_byte_type(),
        })
    }

    pub fn get_associated_stealth_resources(
        &self,
        address: &ComponentAddress,
    ) -> Result<HashSet<ResourceAddress>, AccountsApiError> {
        let resources = self
            .store
            .with_read_tx(|tx| tx.accounts_get_associated_stealth_resources(address))?;
        Ok(resources)
    }

    pub fn get_account_by_public_key(
        &self,
        public_key: &RistrettoPublicKeyBytes,
    ) -> Result<AccountWithAddress, AccountsApiError> {
        let account_address = derive_account_address_from_public_key(public_key);
        let account = self.store.with_read_tx(|tx| tx.accounts_get(&account_address))?;
        let address = self.key_manager_api.derive_account_address(account.key_index)?;
        Ok(AccountWithAddress {
            account,
            address: address.address.to_byte_type(),
        })
    }

    pub fn get_account_or_default(&self, address: Option<&ComponentAddress>) -> Result<Account, AccountsApiError> {
        self.store.with_read_tx(|tx| {
            if let Some(address) = address {
                let account = tx.accounts_get(address)?;
                return Ok(account);
            }
            let account = tx.accounts_get_default()?;
            Ok(account)
        })
    }

    pub fn get_by_vault(&self, vault_addr: &VaultId) -> Result<Account, AccountsApiError> {
        let mut tx = self.store.create_read_tx()?;
        let account = tx.accounts_get_by_vault(vault_addr)?;
        Ok(account)
    }

    pub fn get_vault_by_resource(
        &self,
        account_addr: &ComponentAddress,
        resource_addr: &ResourceAddress,
    ) -> Result<VaultModel, AccountsApiError> {
        let mut tx = self.store.create_read_tx()?;
        let vault = tx.vaults_get_by_resource(account_addr, resource_addr)?;
        Ok(vault)
    }

    pub fn get_vault(&self, vault_addr: &VaultId) -> Result<VaultModel, AccountsApiError> {
        let mut tx = self.store.create_read_tx()?;
        let vault = tx.vaults_get(vault_addr)?;
        Ok(vault)
    }

    pub fn has_account(&self, addr: &ComponentAddress) -> Result<bool, AccountsApiError> {
        let mut tx = self.store.create_read_tx()?;
        // TODO: consider optimising
        let exists = tx.accounts_get(addr).optional()?.is_some();
        Ok(exists)
    }

    pub fn has_vault(&self, vault_id: &VaultId) -> Result<bool, AccountsApiError> {
        let mut tx = self.store.create_read_tx()?;
        let exists = tx.vaults_exists(vault_id)?;
        Ok(exists)
    }

    pub fn rename_account(&self, account_addr: &ComponentAddress, new_name: &str) -> Result<(), AccountsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.accounts_update(account_addr, AccountUpdate {
            name: Some(new_name),
            ..Default::default()
        })?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_default_account(&self, account_addr: &ComponentAddress) -> Result<(), AccountsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.accounts_set_default(account_addr)?;
        tx.commit()?;
        Ok(())
    }

    pub fn add_vault(
        &self,
        account_address: ComponentAddress,
        vault_address: VaultId,
        resource_address: ResourceAddress,
        resource_type: ResourceType,
        token_symbol: Option<String>,
        divisibility: u8,
    ) -> Result<(), AccountsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.vaults_insert(VaultModel {
            account_address,
            id: vault_address,
            resource_address,
            resource_type,
            revealed_balance: Amount::zero(),
            confidential_balance: Amount::zero(),
            locked_revealed_balance: Amount::zero(),
            token_symbol,
            divisibility,
        })?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_account_by_vault(&self, vault_addr: &&VaultId) -> Result<Account, AccountsApiError> {
        let mut tx = self.store.create_read_tx()?;
        let account = tx.accounts_get_by_vault(vault_addr)?;
        Ok(account)
    }

    pub fn get_vaults_by_account(&self, account: &ComponentAddress) -> Result<Vec<VaultModel>, AccountsApiError> {
        let mut tx = self.store.create_read_tx()?;
        let vaults = tx.vaults_get_by_account(account)?;
        Ok(vaults)
    }
}

impl<'a, TStore, TNetworkInterface> AccountsApi<'a, TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError,
{
    pub async fn resolve_account_by_public_key(
        &self,
        destination_pk: &RistrettoPublicKeyBytes,
    ) -> Result<ResolvedAccountDetails, ConfidentialTransferApiError> {
        let account_component = derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, destination_pk);
        // Is it an account we own?
        if let Some(vaults) = self.get_vaults_by_account(&account_component).optional()? {
            let account = self.get_account_by_address(&account_component)?;

            return Ok(ResolvedAccountDetails {
                address: account_component,
                vaults: vaults.into_iter().map(|v| v.id).collect(),
                exists_on_chain: account.is_confirmed_on_chain(),
            });
        }

        match self
            .substates_api
            .fetch_substate_from_network(&account_component.into(), None)
            .await
            .optional()?
        {
            Some(ValidatorScanResult {
                id: address, substate, ..
            }) => {
                let indexed_component = substate
                    .component()
                    .map(|c| IndexedWellKnownTypes::from_value(c.state()))
                    .transpose()
                    .map_err(|e| ConfidentialTransferApiError::UnexpectedIndexerResponse {
                        details: format!("Failed to extract vaults from account component: {e}"),
                    })?
                    .ok_or_else(|| ConfidentialTransferApiError::UnexpectedIndexerResponse {
                        details: format!(
                            "Expected indexer to return a component for account {} but it returned {} (value type: {})",
                            account_component,
                            address,
                            SubstateType::from(&substate)
                        ),
                    })?;
                Ok(ResolvedAccountDetails {
                    address: account_component,
                    vaults: indexed_component.vault_ids().to_vec(),
                    exists_on_chain: true,
                })
            },
            None => Ok(ResolvedAccountDetails {
                address: account_component,
                vaults: vec![],
                exists_on_chain: false,
            }),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AccountsApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Key manager API error: {0}")]
    KeyManagerApiError(#[from] KeyManagerApiError),
    #[error("Account name already exists: {name}")]
    AccountNameAlreadyExists { name: String },
}

impl AccountsApiError {
    pub fn is_name_exists_error(&self) -> bool {
        matches!(self, Self::AccountNameAlreadyExists { .. })
    }
}

impl IsNotFoundError for AccountsApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::StoreError(e) if e.is_not_found_error() )
    }
}
