//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use log::*;
use tari_dan_common_types::{
    displayable::Displayable,
    optional::{IsNotFoundError, Optional},
    substate_type::SubstateType,
    SubstateRequirement,
    VersionedSubstateId,
};
use tari_engine_types::{
    indexed_value::{IndexedValueError, IndexedWellKnownTypes},
    substate::{SubstateId, SubstateValue},
    transaction_receipt::TransactionReceiptAddress,
    TemplateAddress,
};
use tari_template_lib::constants::XTR;
use tari_transaction::TransactionId;

use crate::{
    models::SubstateModel,
    network::WalletNetworkInterface,
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

const LOG_TARGET: &str = "tari::dan::wallet_sdk::apis::substate";

pub struct SubstatesApi<'a, TStore, TNetworkInterface> {
    store: &'a TStore,
    network_interface: &'a TNetworkInterface,
}

impl<'a, TStore, TNetworkInterface> SubstatesApi<'a, TStore, TNetworkInterface>
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

    pub fn get_substate(&self, address: &SubstateId) -> Result<SubstateModel, SubstateApiError> {
        let mut tx = self.store.create_read_tx()?;
        let substate = tx.substates_get(address)?;
        Ok(substate)
    }

    pub fn list_substates(
        &self,
        filter_by_type: Option<SubstateType>,
        filter_by_template: Option<&TemplateAddress>,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<SubstateModel>, SubstateApiError> {
        let mut tx = self.store.create_read_tx()?;
        let substates = tx.substates_get_all(filter_by_type, filter_by_template, limit, offset)?;
        Ok(substates)
    }

    pub fn load_dependent_substates(
        &self,
        parents: &[&SubstateId],
    ) -> Result<HashSet<SubstateRequirement>, SubstateApiError> {
        let mut substate_ids = HashSet::with_capacity(parents.len());
        let mut tx = self.store.create_read_tx()?;
        for parent_addr in parents {
            let parent = tx.substates_get(parent_addr)?;
            get_dependent_substates(&mut tx, parent, &mut substate_ids)?;
        }
        Ok(substate_ids)
    }

    #[allow(clippy::too_many_lines)]
    pub async fn locate_dependent_substates(
        &self,
        parents: &[SubstateId],
    ) -> Result<HashSet<SubstateRequirement>, SubstateApiError> {
        let mut substate_ids = HashSet::with_capacity(parents.len());

        for parent_id in parents {
            match self.store.with_read_tx(|tx| tx.substates_get(parent_id)).optional()? {
                Some(parent) => {
                    debug!(
                        target: LOG_TARGET,
                        "Parent substate {} found in store, loading dependent substates",
                        parent.substate_id
                    );
                    self.store
                        .with_read_tx(|tx| get_dependent_substates(tx, parent, &mut substate_ids))?;
                },
                None => {
                    debug!(
                        target: LOG_TARGET,
                        "Parent substate {} not found in store, requesting dependent substates",
                        parent_id
                    );
                    let ValidatorScanResult {
                        address: substate_id,
                        substate,
                        ..
                    } = self.scan_for_substate(parent_id, None).await?;

                    match &substate {
                        SubstateValue::Component(data) => {
                            let value = IndexedWellKnownTypes::from_value(&data.body.state)?;
                            for addr in value.referenced_substates() {
                                if substate_ids.contains(&addr) {
                                    continue;
                                }

                                let ValidatorScanResult { address: addr, .. } =
                                    self.scan_for_substate(&addr, None).await?;
                                substate_ids.insert(addr.into());
                            }
                        },
                        SubstateValue::Resource(_) => {},
                        SubstateValue::TransactionReceipt(tx_receipt) => {
                            let tx_receipt_addr = SubstateId::TransactionReceipt(TransactionReceiptAddress::from_hash(
                                tx_receipt.transaction_hash,
                            ));
                            if substate_ids.contains(&tx_receipt_addr) {
                                continue;
                            }
                            let ValidatorScanResult { address: id, .. } =
                                self.scan_for_substate(&tx_receipt_addr, None).await?;
                            substate_ids.insert(id.into());
                        },
                        SubstateValue::Vault(vault) => {
                            let resx_addr = SubstateId::Resource(*vault.resource_address());
                            if substate_ids.contains(&resx_addr) {
                                continue;
                            }
                            let ValidatorScanResult { address: id, .. } =
                                self.scan_for_substate(&resx_addr, None).await?;
                            substate_ids.insert(id.into());
                        },
                        SubstateValue::NonFungible(_) => {
                            let nft_addr = substate_id.substate_id().as_non_fungible_address().ok_or_else(|| {
                                SubstateApiError::InvalidValidatorNodeResponse(format!(
                                    "NonFungible substate does not have a valid address {}",
                                    substate_id
                                ))
                            })?;

                            let resx_addr = SubstateId::Resource(*nft_addr.resource_address());
                            if substate_ids.contains(&resx_addr) {
                                continue;
                            }
                            let ValidatorScanResult { address: id, .. } =
                                self.scan_for_substate(&resx_addr, None).await?;
                            substate_ids.insert(id.into());
                        },
                        SubstateValue::NonFungibleIndex(addr) => {
                            let resx_addr = SubstateId::Resource(*addr.referenced_address().resource_address());
                            if substate_ids.contains(&resx_addr) {
                                continue;
                            }
                            let ValidatorScanResult { address: id, .. } =
                                self.scan_for_substate(&resx_addr, None).await?;
                            substate_ids.insert(id.into());
                        },
                        SubstateValue::ValidatorFeePool(_) => {
                            let resx_addr = SubstateId::Resource(XTR);
                            if substate_ids.contains(&resx_addr) {
                                continue;
                            }
                        },
                        SubstateValue::UnclaimedConfidentialOutput(_) => {},
                        SubstateValue::Template(_) => {},
                    }

                    debug!(
                        target: LOG_TARGET,
                        "Adding substate {} to dependent substates from remote source",
                        substate_id
                    );
                    substate_ids.insert(substate_id.into());
                },
            }
        }

        Ok(substate_ids)
    }

    pub async fn scan_for_substate(
        &self,
        address: &SubstateId,
        version_hint: Option<u32>,
    ) -> Result<ValidatorScanResult, SubstateApiError> {
        debug!(
            target: LOG_TARGET,
            "Scanning for substate {} at version {}",
            address,
            version_hint.display()
        );

        let resp = self
            .network_interface
            .query_substate(address, version_hint, false)
            .await
            .optional()
            .map_err(|e| SubstateApiError::NetworkIndexerError(e.into()))?
            .ok_or_else(|| SubstateApiError::SubstateDoesNotExist {
                address: address.clone(),
            })?;

        debug!(
            target: LOG_TARGET,
            "Found substate {} at version {}", address, resp.version
        );
        Ok(ValidatorScanResult {
            address: VersionedSubstateId::new(address.clone(), resp.version),
            created_by_tx: resp.created_by_transaction,
            substate: resp.substate,
        })
    }

    pub fn save_root<I: IntoIterator<Item = SubstateId>>(
        &self,
        created_by_tx: TransactionId,
        address: VersionedSubstateId,
        referenced_substates: I,
    ) -> Result<(), SubstateApiError> {
        self.store.with_write_tx(|tx| {
            let maybe_removed = tx.substates_remove(address.substate_id()).optional()?;
            tx.substates_upsert_root(
                created_by_tx,
                address,
                referenced_substates.into_iter().collect(),
                maybe_removed.as_ref().and_then(|s| s.module_name.clone()),
                maybe_removed.and_then(|s| s.template_address),
            )
        })?;
        Ok(())
    }

    pub fn save_child<I: IntoIterator<Item = SubstateId>>(
        &self,
        created_by_tx: TransactionId,
        parent: SubstateId,
        child: VersionedSubstateId,
        referenced_substates: I,
    ) -> Result<(), SubstateApiError> {
        self.store.with_write_tx(|tx| {
            let maybe_substate = tx.substates_remove(child.substate_id()).optional()?;
            tx.substates_upsert_child(
                created_by_tx,
                parent,
                child,
                maybe_substate
                    .into_iter()
                    .flat_map(|s| s.referenced_substates)
                    .chain(referenced_substates)
                    .collect(),
            )
        })?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SubstateApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Network network_interface error: {0}")]
    NetworkIndexerError(anyhow::Error),
    #[error("Invalid validator node response: {0}")]
    InvalidValidatorNodeResponse(String),
    #[error("Substate {address} does not exist")]
    SubstateDoesNotExist { address: SubstateId },
    #[error("ValueVisitorError: {0}")]
    ValueVisitorError(#[from] IndexedValueError),
}

impl IsNotFoundError for SubstateApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::SubstateDoesNotExist { .. }) ||
            matches!(self, Self::StoreError(e) if e.is_not_found_error())
    }
}

pub struct ValidatorScanResult {
    pub address: VersionedSubstateId,
    pub created_by_tx: TransactionId,
    pub substate: SubstateValue,
}

fn get_dependent_substates<TTx: WalletStoreReader>(
    tx: &mut TTx,
    parent: SubstateModel,
    substate_ids: &mut HashSet<SubstateRequirement>,
) -> Result<(), WalletStorageError> {
    // TODO: this was done to just quickly get things to work but could cause endless recursion or have other bugs -
    // time should be taken to improve this.
    substate_ids.insert(parent.substate_id.clone().into());
    for child in parent.referenced_substates {
        get_dependent_substates_inner(tx, &child, substate_ids)?;
    }

    let children = tx.substates_get_children(parent.substate_id.substate_id())?;
    for child in children {
        if let Some(addr) = child.substate_id.substate_id().as_non_fungible_address() {
            // Ensure that the associated resource is also included
            substate_ids.insert(SubstateRequirement::unversioned(*addr.resource_address()));
        }
        debug!(
            target: LOG_TARGET,
            "Adding substate {} to dependent substates",
            child.substate_id
        );
        substate_ids.insert(child.substate_id.into());
        for child in child.referenced_substates {
            get_dependent_substates_inner(tx, &child, substate_ids)?;
        }
    }
    Ok(())
}
fn get_dependent_substates_inner<TTx: WalletStoreReader>(
    tx: &mut TTx,
    id: &SubstateId,
    substate_ids: &mut HashSet<SubstateRequirement>,
) -> Result<(), WalletStorageError> {
    let Some(substate) = tx.substates_get(id).optional()? else {
        return Ok(());
    };
    debug!(
        target: LOG_TARGET,
        "Adding substate {} to dependent substates",
        substate.substate_id
    );
    substate_ids.insert(substate.substate_id.into());

    for child in substate.referenced_substates {
        if let Some(addr) = child.as_non_fungible_address() {
            debug!(
                target: LOG_TARGET,
                "Adding substate {} to dependent substates",
                child
            );
            // Ensure that the associated resource is also included
            substate_ids.insert(SubstateRequirement::unversioned(*addr.resource_address()));
        }

        if substate_ids.contains(&child) {
            continue;
        }

        get_dependent_substates_inner(tx, &child, substate_ids)?;
        debug!(
            target: LOG_TARGET,
            "Adding substate {} to dependent substates",
            child
        );
        substate_ids.insert(child.into());
    }
    Ok(())
}
