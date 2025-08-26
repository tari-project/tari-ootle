//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_engine_types::resource::Resource;
use tari_ootle_common_types::optional::IsNotFoundError;
use tari_template_lib::{models::ResourceAddress, prelude::ResourceType};
use thiserror::Error;

use crate::storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter};

pub struct ResourcesApi<'db, TStore> {
    store: &'db TStore,
}

impl<'db, TStore> ResourcesApi<'db, TStore>
where TStore: WalletStore
{
    pub fn new(store: &'db TStore) -> Self {
        Self { store }
    }

    pub fn upsert_resource(&self, address: &ResourceAddress, resource: &Resource) -> Result<(), ResourcesApiError> {
        self.store.with_write_tx(|tx| tx.resources_upsert(address, resource))?;
        Ok(())
    }

    pub fn get(&self, address: &ResourceAddress) -> Result<Resource, ResourcesApiError> {
        let resource = self.store.with_read_tx(|tx| tx.resources_get(address))?;
        Ok(resource.into())
    }

    pub fn get_addresses_by_type(
        &self,
        resource_type: ResourceType,
    ) -> Result<Vec<ResourceAddress>, ResourcesApiError> {
        let resources = self.store.with_read_tx(|tx| tx.resources_get_by_type(resource_type))?;
        Ok(resources.into_iter().map(|model| model.address).collect())
    }

    pub fn get_many<'a, I: IntoIterator<Item = &'a ResourceAddress>>(
        &self,
        addresses: I,
    ) -> Result<HashMap<ResourceAddress, Resource>, ResourcesApiError> {
        let resources = self.store.with_read_tx(|tx| tx.resources_get_many(addresses))?;
        Ok(resources
            .into_iter()
            .map(|model| (model.address, model.resource))
            .collect())
    }
}

#[derive(Debug, Error)]
pub enum ResourcesApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
}

impl IsNotFoundError for ResourcesApiError {
    fn is_not_found_error(&self) -> bool {
        match self {
            ResourcesApiError::StoreError(err) => err.is_not_found_error(),
        }
    }
}
