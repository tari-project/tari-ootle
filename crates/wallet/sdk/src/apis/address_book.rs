//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::optional::IsNotFoundError;
use thiserror::Error;

use crate::{
    models::AddressBookEntry,
    storage::{CommittableStore, WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

pub struct AddressBookApi<'a, TStore> {
    store: &'a TStore,
}

impl<'a, TStore> AddressBookApi<'a, TStore>
where TStore: WalletStore
{
    pub fn new(store: &'a TStore) -> Self {
        Self { store }
    }

    pub fn add(
        &self,
        name: &str,
        address: &str,
        memo: Option<&str>,
    ) -> Result<AddressBookEntry, AddressBookApiError> {
        let mut tx = self.store.create_write_tx()?;
        let entry = tx.address_book_insert(name, address, memo)?;
        tx.commit()?;
        Ok(entry)
    }

    pub fn get(&self, name: &str) -> Result<AddressBookEntry, AddressBookApiError> {
        let mut tx = self.store.create_read_tx()?;
        let entry = tx.address_book_get(name)?;
        Ok(entry)
    }

    pub fn list(&self) -> Result<Vec<AddressBookEntry>, AddressBookApiError> {
        let mut tx = self.store.create_read_tx()?;
        let entries = tx.address_book_get_all()?;
        Ok(entries)
    }

    pub fn update(
        &self,
        name: &str,
        new_name: Option<&str>,
        address: Option<&str>,
        memo: Option<&str>,
    ) -> Result<AddressBookEntry, AddressBookApiError> {
        let mut tx = self.store.create_write_tx()?;
        let entry = tx.address_book_update(name, new_name, address, memo)?;
        tx.commit()?;
        Ok(entry)
    }

    pub fn delete(&self, name: &str) -> Result<(), AddressBookApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.address_book_delete(name)?;
        tx.commit()?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum AddressBookApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
}

impl IsNotFoundError for AddressBookApiError {
    fn is_not_found_error(&self) -> bool {
        match self {
            AddressBookApiError::StoreError(err) => err.is_not_found_error(),
        }
    }
}
