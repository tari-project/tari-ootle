//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use tari_dan_common_types::optional::IsNotFoundError;
use tari_template_lib::{
    models::{ResourceAddress, VaultId},
    prelude::{ComponentAddress, NonFungibleId},
};
use thiserror::Error;

use crate::{
    models::NonFungibleToken,
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

pub struct NonFungibleTokensApi<'a, TStore> {
    store: &'a TStore,
}

impl<'a, TStore> NonFungibleTokensApi<'a, TStore>
where TStore: WalletStore
{
    pub fn new(store: &'a TStore) -> Self {
        Self { store }
    }

    pub fn save_nft(&self, non_fungible: &NonFungibleToken) -> Result<(), NonFungibleTokensApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.non_fungible_token_upsert(non_fungible)?;
        tx.commit()?;
        Ok(())
    }

    pub fn remove_nft(&self, vault_id: &VaultId, nft_id: &NonFungibleId) -> Result<(), NonFungibleTokensApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.non_fungible_token_remove(vault_id, nft_id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_by_id(&self, nft_id: NonFungibleId) -> Result<NonFungibleToken, NonFungibleTokensApiError> {
        let mut tx = self.store.create_read_tx()?;
        let non_fungible_token = tx.non_fungible_token_get_by_nft_id(nft_id)?;
        Ok(non_fungible_token)
    }

    pub fn get_nft_ids_by_vault_id(
        &self,
        vault_id: &VaultId,
        limit: u64,
        offset: u64,
    ) -> Result<HashSet<NonFungibleId>, NonFungibleTokensApiError> {
        let mut tx = self.store.create_read_tx()?;
        let non_fungibles = tx.non_fungible_token_get_ids_by_vault_id(vault_id, limit, offset)?;
        Ok(non_fungibles)
    }

    pub fn get_all(
        &self,
        account: ComponentAddress,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<NonFungibleToken>, NonFungibleTokensApiError> {
        let mut tx = self.store.create_read_tx()?;
        let non_fungibles = tx.non_fungible_token_get_all(account, limit, offset)?;
        Ok(non_fungibles)
    }

    pub fn get_resource_address(&self, nft_id: NonFungibleId) -> Result<ResourceAddress, NonFungibleTokensApiError> {
        let mut tx = self.store.create_read_tx()?;
        let resource_address = tx.non_fungible_token_get_resource_address(nft_id)?;
        Ok(resource_address)
    }
}

#[derive(Debug, Error)]
pub enum NonFungibleTokensApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
}

impl IsNotFoundError for NonFungibleTokensApiError {
    fn is_not_found_error(&self) -> bool {
        match self {
            NonFungibleTokensApiError::StoreError(err) => err.is_not_found_error(),
        }
    }
}
