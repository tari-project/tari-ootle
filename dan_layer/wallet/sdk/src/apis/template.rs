// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::optional::IsNotFoundError;
use tari_engine_types::TemplateAddress;

use crate::{
    apis::transaction::TransactionApiError,
    models::AuthoredTemplateModel,
    network::WalletNetworkInterface,
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

const LOG_TARGET: &str = "tari::dan::wallet_sdk::apis::template";

pub struct TemplateApi<'a, TStore, TNetworkInterface> {
    store: &'a TStore,
    network_interface: &'a TNetworkInterface,
}

impl<'a, TStore, TNetworkInterface> TemplateApi<'a, TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError,
{
    pub fn new(store: &'a TStore, network_interface: &'a TNetworkInterface) -> Self {
        Self {
            store,
            network_interface,
        }
    }

    /// Adds a new template to the list of known templates authored by an owned account.
    /// If the template already exists in this list, will do nothing, just return success.
    pub async fn add_authored_template(
        &self,
        key_index: u64,
        template_address: TemplateAddress,
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

        // save new template
        let template_definition = self
            .network_interface
            .fetch_template_definition(template_address)
            .await
            .map_err(|error| TransactionApiError::NetworkInterfaceError(format!("{}", error)))?;
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
