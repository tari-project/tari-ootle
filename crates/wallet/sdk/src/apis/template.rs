// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::TemplateDef;
use tari_template_lib::types::{TemplateAddress, crypto::RistrettoPublicKeyBytes};

use crate::{
    apis::transaction::TransactionApiError,
    models::AuthoredTemplateModel,
    storage::{WalletStore, WalletStoreReader, WalletStoreWriter},
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
    pub fn add_authored_template(
        &self,
        author_public_key: RistrettoPublicKeyBytes,
        template_address: TemplateAddress,
        template_definition: TemplateDef,
    ) -> Result<(), TransactionApiError> {
        self.store.with_write_tx(|tx| {
            tx.authored_templates_insert(AuthoredTemplateModel::new(
                author_public_key,
                template_address,
                template_definition,
            ))
        })?;
        Ok(())
    }

    pub fn template_exists(&self, template_address: TemplateAddress) -> Result<bool, TransactionApiError> {
        let exists = self
            .store
            .with_read_tx(|tx| tx.authored_templates_exists_by_address(&template_address))?;
        Ok(exists)
    }

    pub fn fetch_authored_template(
        &self,
        template_address: TemplateAddress,
    ) -> Result<AuthoredTemplateModel, TransactionApiError> {
        let template = self
            .store
            .with_read_tx(|tx| tx.authored_templates_get_by_address(&template_address))?;
        Ok(template)
    }

    /// Listing authored templates in a paginated way.
    pub fn list_templates(
        &self,
        author_public_key: Option<&RistrettoPublicKeyBytes>,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<AuthoredTemplateModel>, u64), TransactionApiError> {
        Ok(self
            .store
            .with_read_tx(|tx| tx.authored_templates_get_many(author_public_key, page, page_size))?)
    }
}
