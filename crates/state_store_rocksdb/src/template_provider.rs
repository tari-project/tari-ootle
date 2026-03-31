//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::published_template::{PublishedTemplate, PublishedTemplateAddress};
use tari_ootle_common_types::{SubstateAddress, services::template_provider::TemplateProvider};
use tari_ootle_storage::{Ordering, StorageError};
use tari_template_lib_types::TemplateAddress;

use crate::{
    RocksDbStateStore,
    column_families::{substate, substate::SubstateCf, template_metadata::TemplateMetadataCf},
};

impl<TAddr: Send + Sync + 'static> TemplateProvider for RocksDbStateStore<TAddr> {
    type Error = StorageError;
    type Template = PublishedTemplate;

    fn get_template(&self, id: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
        const OPERATION: &str = "RocksDbReadOnlyStateStore::get_template";
        let cx = self.snapshot();
        let cf = cx.cf(SubstateCf)?;
        let address = template_address_to_substate_address(*id);
        let substate = cf.get(&address, OPERATION)?;
        let value = substate.into_substate_value().ok_or_else(|| StorageError::NotFound {
            item: "template substate",
            key: format!("Template substate not found: {}", address),
        })?;

        let template = value.into_template().ok_or_else(|| StorageError::DataInconsistency {
            details: format!("Template substate does not contain a published template: {}", address),
        })?;

        Ok(Some(template))
    }

    fn has_template(&self, id: &TemplateAddress) -> Result<bool, Self::Error> {
        const OPERATION: &str = "RocksDbReadOnlyStateStore::has_template";
        let cx = self.snapshot();
        let cf = cx.cf(SubstateCf)?;
        let address = template_address_to_substate_address(*id);
        let exists = cf.exists(&address, OPERATION)?;
        Ok(exists)
    }
}

impl<TAddr: Send + Sync + 'static> RocksDbStateStore<TAddr> {
    /// Scans the substate head index and returns the addresses of all currently *up* (live)
    /// template substates that are **missing** from `TemplateMetadataCf`.
    ///
    /// Called once at validator-node startup to backfill metadata for templates that were
    /// published before this code was deployed. On subsequent restarts, templates that already
    /// have metadata entries are skipped, keeping the operation near-instant.
    pub fn scan_template_addresses_missing_metadata(&self) -> Result<Vec<TemplateAddress>, StorageError> {
        const OPERATION: &str = "scan_template_addresses_missing_metadata";
        let cx = self.snapshot();
        let head_index = cx.cf(substate::HeadIndex)?;
        let metadata_cf = cx.cf(TemplateMetadataCf)?;
        let mut addresses = Vec::new();
        for result in head_index.iterator(Ordering::Ascending, OPERATION) {
            let (id, data) = result?;
            if data.is_up &&
                let Some(published) = id.as_template()
            {
                let addr = published.as_template_address();
                if !metadata_cf.exists(&addr, OPERATION)? {
                    addresses.push(addr);
                }
            }
        }
        Ok(addresses)
    }
}

fn template_address_to_substate_address(address: TemplateAddress) -> SubstateAddress {
    let address = PublishedTemplateAddress::from_hash(address);
    SubstateAddress::from_object_key(address.as_object_key(), 0)
}
