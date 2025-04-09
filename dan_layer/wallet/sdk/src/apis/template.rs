// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::TemplateDef;
use tari_template_lib::types::TemplateAddress;

use crate::{
    apis::transaction::TransactionApiError,
    models::AuthoredTemplateModel,
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

pub struct TemplateApi<'a, TStore> {
    store: &'a TStore,
}

impl<'a, TStore> TemplateApi<'a, TStore>
where TStore: WalletStore
{
    pub fn new(store: &'a TStore) -> Self {
        Self { store }
    }

    /// Adds a new template to the list of known templates authored by an owned account.
    /// If the template already exists in this list, will do nothing, just return success.
    pub async fn add_authored_template(
        &self,
        key_index: u64,
        template_address: TemplateAddress,
        template_definition: TemplateDef,
    ) -> Result<(), TransactionApiError> {
        // check if we already have this template
        if self.store.with_read_tx(|tx| {
            if tx.authored_templates_exists_by_address(&template_address)? {
                return Ok::<bool, WalletStorageError>(true);
            }
            Ok(false)
        })? {
            return Ok(());
        };

        self.store.with_write_tx(|tx| {
            tx.authored_templates_insert(AuthoredTemplateModel::new(
                key_index,
                template_address,
                template_definition,
            ))?;
            Ok(())
        })
    }

    /// Listing authored templates in a paginated way.
    pub fn list_authored_templates(
        &self,
        key_index: u64,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<AuthoredTemplateModel>, u64), TransactionApiError> {
        Ok(self
            .store
            .with_read_tx(|tx| tx.authored_templates_fetch_by_key_index(key_index, page, page_size))?)
    }
}
