//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{HashMap, HashSet};

use log::*;
use tari_engine_types::{
    indexed_value::{IndexedValueError, IndexedWellKnownTypes},
    substate::{SubstateDiff, SubstateId},
};
use tari_ootle_common_types::{
    VersionedSubstateIdRef,
    optional::{IsNotFoundError, Optional},
    response_status::{ResponseErrorStatus, TransactionStatusResponseError},
};
use tari_ootle_transaction::{Transaction, TransactionId};
use tari_template_lib::types::{ComponentAddress, constants::TARI_TOKEN, crypto::RistrettoPublicKeyBytes};

use crate::{
    models::{NewAccountData, TransactionStatus, WalletLockId, WalletTransaction, WalletTransactionUpdate},
    network::{TransactionFinalizedResult, WalletNetworkInterface},
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};

const LOG_TARGET: &str = "tari::ootle::wallet_sdk::apis::transaction";

pub struct TransactionApi<'a, TStore, TNetworkInterface> {
    store: &'a TStore,
    network_interface: &'a TNetworkInterface,
}

impl<'a, TStore, TNetworkInterface> TransactionApi<'a, TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError + TransactionStatusResponseError,
{
    pub fn new(store: &'a TStore, network_interface: &'a TNetworkInterface) -> Self {
        Self {
            store,
            network_interface,
        }
    }

    pub fn get(&self, tx_id: TransactionId) -> Result<WalletTransaction, TransactionApiError> {
        let mut tx = self.store.create_read_tx()?;
        let transaction = tx.transactions_get(tx_id)?;
        Ok(transaction)
    }

    /// Inserts a new transaction into the wallet database with status `New`.
    pub fn insert_new_transaction(
        &self,
        transaction: Transaction,
        new_account_info: Option<NewAccountData>,
        is_dry_run: bool,
    ) -> Result<TransactionId, TransactionApiError> {
        let tx_id = transaction.calculate_id();
        self.store
            .with_write_tx(|tx| tx.transactions_insert(&transaction, new_account_info.as_ref(), is_dry_run))?;

        Ok(tx_id)
    }

    /// Submits a transaction to the network. The transaction must be in the `New` status.
    /// If the submission is successful, the transaction status is updated to `Pending`.
    /// If the transaction is rejected, the status is updated to `InvalidTransaction` and the
    /// rejection reason is stored.
    /// Returns `Ok(true)` if the transaction was successfully submitted, `Ok(false)` if it was rejected
    pub async fn submit_transaction(&self, transaction_id: TransactionId) -> Result<bool, TransactionApiError> {
        let transaction = self.store.with_read_tx(|tx| tx.transactions_get(transaction_id))?;
        if transaction.is_dry_run {
            return Err(TransactionApiError::DryRunMismatchError {
                details: "Transaction is marked as dry run and cannot be submitted".to_string(),
            });
        }

        if !matches!(transaction.status, TransactionStatus::New) {
            return Err(TransactionApiError::StoreError(WalletStorageError::OperationError {
                operation: "submit_transaction",
                details: format!("Transaction {} is not in New status", transaction_id),
            }));
        }

        // Re-submission needs the full transaction (blob payloads); the WalletTransaction
        // returned above is the pruned API view.
        let full = self.store.with_read_tx(|tx| tx.transactions_get_full(transaction_id))?;
        let resp = self.network_interface.submit_transaction(full).await;

        match resp {
            Ok(_) => {
                self.store.with_write_tx(|tx| {
                    tx.transactions_update(
                        WalletTransactionUpdate::new(transaction_id).with_new_status(TransactionStatus::Pending),
                    )
                })?;
            },
            Err(err) => {
                return match err.get_status() {
                    ResponseErrorStatus::TransactionRejected { message } => {
                        warn!(target: LOG_TARGET, "Invalid transaction submission: {transaction_id} {message}");
                        self.store.with_write_tx(|tx| {
                            tx.transactions_update(
                                WalletTransactionUpdate::new(transaction_id)
                                    .with_new_status(TransactionStatus::InvalidTransaction)
                                    .with_invalid_reason(&message),
                            )
                        })?;
                        Ok(false)
                    },
                    _ => Err(err.into()),
                };
            },
        }

        Ok(true)
    }

    pub async fn submit_dry_run_transaction(
        &self,
        transaction: Transaction,
    ) -> Result<WalletTransaction, TransactionApiError> {
        if !transaction.is_dry_run() {
            return Err(TransactionApiError::DryRunMismatchError {
                details: "Transaction is not marked as dry run but submitted to dry-run".to_string(),
            });
        }

        self.store
            .with_write_tx(|tx| tx.transactions_insert(&transaction, None, true))?;

        let tx_id = transaction.calculate_id();
        let result = self.network_interface.submit_dry_run_transaction(transaction).await;

        match result {
            Ok(query) => match &query.result {
                TransactionFinalizedResult::Pending => {
                    return Err(TransactionApiError::InvalidTransactionQueryResponse {
                        details: "Pending execution result returned from dry run".to_string(),
                    });
                },
                TransactionFinalizedResult::Finalized {
                    execution_result,
                    finalized_time,
                    execution_time,
                    ..
                } => {
                    if query.transaction_id != tx_id {
                        // This could indicate that there has been some breaking change that caused the indexer to
                        // calculate a different transaction ID
                        warn!(target: LOG_TARGET, "⚠️ Transaction ID mismatch in dry run response. Expected {}, got {}. Updating transaction status to DryRunFailed.", tx_id, query.transaction_id);
                    }

                    self.store.with_write_tx(|tx| {
                        tx.transactions_update(
                            WalletTransactionUpdate::new(tx_id)
                                .with_result(execution_result.as_ref().map(|e| &e.finalize))
                                .with_final_fee(
                                    execution_result
                                        .as_ref()
                                        .map(|e| e.finalize.fee_receipt.required_fees()),
                                )
                                .with_new_status(TransactionStatus::DryRun)
                                .with_execution_time(*execution_time)
                                .with_finalized_time(*finalized_time),
                        )
                    })?;
                },
            },
            Err(err) => {
                self.store.with_write_tx(|tx| {
                    tx.transactions_update(
                        WalletTransactionUpdate::new(tx_id).with_new_status(TransactionStatus::DryRunFailed),
                    )
                })?;
                return Err(err.into());
            },
        }

        let transaction = self.store.with_read_tx(|tx| tx.transactions_get(tx_id))?;

        Ok(transaction)
    }

    pub fn fetch_all(
        &self,
        status: Option<TransactionStatus>,
        component: Option<ComponentAddress>,
        signed_by_public_key: Option<RistrettoPublicKeyBytes>,
    ) -> Result<Vec<WalletTransaction>, TransactionApiError> {
        let mut tx = self.store.create_read_tx()?;
        let transactions = tx.transactions_fetch_all(status, component, signed_by_public_key)?;
        Ok(transactions)
    }

    pub async fn check_and_store_finalized_transaction(
        &self,
        transaction_id: TransactionId,
    ) -> Result<Option<WalletTransaction>, TransactionApiError> {
        // Multithreaded considerations: The transaction result could be requested more than once because db
        // transactions cannot be used around await points.
        let transaction = self.store.with_read_tx(|tx| tx.transactions_get(transaction_id))?;
        if transaction.finalize.is_some() {
            return Ok(Some(transaction));
        }

        let maybe_resp = self
            .network_interface
            .query_transaction_result(transaction_id)
            .await
            .optional()?;

        let Some(resp) = maybe_resp else {
            // TODO: if this happens forever we might want to resubmit or mark as invalid
            warn!( target: LOG_TARGET, "Transaction result not found for transaction with hash {}. Will check again later.", transaction_id);
            return Ok(None);
        };

        match resp.result {
            TransactionFinalizedResult::Pending => Ok(None),
            TransactionFinalizedResult::Finalized {
                final_decision,
                execution_result,
                execution_time,
                finalized_time,
                abort_details: _,
                ..
            } => {
                let new_status = if final_decision.is_commit() {
                    match execution_result.as_ref() {
                        Some(execution_result) => {
                            if execution_result.finalize.is_fee_only() {
                                TransactionStatus::OnlyFeeAccepted
                            } else {
                                TransactionStatus::Accepted
                            }
                        },
                        None => TransactionStatus::Accepted,
                    }
                } else {
                    TransactionStatus::Rejected
                };

                // let qc_resp = self.network_interface
                //     .fetch_transaction_quorum_certificates(GetTransactionQcsRequest { hash })
                //     .await
                //     .map_err(TransactionApiError::ValidatorNodeClientError)?;

                let transaction = self.store.with_write_tx(|tx| {
                    if !transaction.is_dry_run && final_decision.is_commit() {
                        let diff = execution_result
                            .as_ref()
                            .and_then(|e| e.finalize.result.any_accept())
                            .ok_or_else(|| TransactionApiError::InvalidTransactionQueryResponse {
                                details: format!(
                                    "NEVERHAPPEN: Finalize decision is COMMIT but transaction failed: {:?}",
                                    execution_result.as_ref().and_then(|e| e.finalize.result.fee_reject())
                                ),
                            })?;

                        self.commit_diff(tx, diff)?;
                    }

                    tx.transactions_update(
                        WalletTransactionUpdate::new(transaction_id)
                            .with_result(execution_result.as_ref().map(|e| &e.finalize))
                            .with_final_fee(
                                execution_result
                                    .as_ref()
                                    .map(|e| e.finalize.fee_receipt.total_fees_charged()),
                            )
                            .with_new_status(new_status)
                            .with_execution_time(execution_time)
                            .with_finalized_time(finalized_time),
                    )?;


                    // Make sure that any locked outputs are either set to spent or released, depending on if the
                    // transaction was finalized or rejected. Always release for dry runs.
                    if transaction.is_dry_run {
                        self.release_all_locks_for_transaction_internal(tx, transaction_id)?;
                    } else {
                        let maybe_diff = execution_result
                            .as_ref()
                            .and_then(|e| e.finalize.result.any_accept());
                        match maybe_diff {
                            Some(diff) => {
                                if let Some(lock_id) = tx.locks_get_by_transaction_id(transaction_id).optional()? {
                                    info!(target: LOG_TARGET, "Finalizing locked outputs for transaction {}: {}", transaction_id, lock_id);
                                    tx.locks_unlock_finalized(lock_id, diff)?;
                                }
                            }
                            None => {
                                self.release_all_locks_for_transaction_internal(tx, transaction_id)?;
                            }
                        }
                    }

                    let transaction = tx.transactions_get(transaction_id)?;
                    Ok::<_, TransactionApiError>(transaction)
                })?;

                Ok(Some(transaction))
            },
        }
    }

    pub fn release_all_locks_for_transaction(&self, transaction_id: TransactionId) -> Result<(), TransactionApiError> {
        self.store
            .with_write_tx(|tx| self.release_all_locks_for_transaction_internal(tx, transaction_id))
    }

    pub fn locks_set_transaction_id(
        &self,
        lock_id: WalletLockId,
        transaction_id: TransactionId,
    ) -> Result<(), TransactionApiError> {
        self.store
            .with_write_tx(|tx| tx.locks_link_transaction(lock_id, transaction_id))?;
        Ok(())
    }

    fn release_all_locks_for_transaction_internal(
        &self,
        tx: &mut <TStore as WriteableWalletStore>::WriteTransaction<'_>,
        transaction_id: TransactionId,
    ) -> Result<(), TransactionApiError> {
        if let Some(lock_id) = tx.locks_get_by_transaction_id(transaction_id).optional()? {
            debug!(target: LOG_TARGET, "Releasing lock {} (and associated outputs) for transaction {} that was not committed", lock_id, transaction_id);
            tx.locks_release(lock_id)?;
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn commit_diff(
        &self,
        tx: &mut TStore::WriteTransaction<'_>,
        diff: &SubstateDiff,
    ) -> Result<(), TransactionApiError> {
        let mut downed_substates_with_parents = HashMap::with_capacity(diff.down_len());
        for (id, _) in diff.down_iter() {
            if id.is_claimed_output_tombstone() {
                // Should never happen
                warn!(target: LOG_TARGET, "❓️ Claimed tombstone {} downed", id);
                continue;
            }

            let Some(downed) = tx.substates_remove(id).optional()? else {
                debug!(target: LOG_TARGET, "Downed substate {} not found", id);
                continue;
            };

            if let Some(parent) = downed.parent_address {
                downed_substates_with_parents.insert(downed.substate_id.into_substate_id(), parent);
            }
        }

        let (components, mut other_substates) = diff.up_iter().partition::<Vec<_>, _>(|(addr, _)| addr.is_component());

        for (component_addr, substate) in components {
            let component = substate.substate_value().component().unwrap();
            let indexed =
                IndexedWellKnownTypes::from_value(component.state()).map_err(TransactionApiError::IndexedValueError)?;

            debug!(target: LOG_TARGET, "Substate {} up", component_addr);
            tx.substates_upsert_root(
                VersionedSubstateIdRef::new(component_addr, substate.version()),
                indexed.referenced_substates().collect(),
                Some(component.module_name.clone()),
                Some(component.template_address),
            )?;

            for owned_id in indexed.referenced_substates() {
                if let Some(pos) = other_substates.iter().position(|(addr, _)| *addr == owned_id) {
                    let (_, child) = other_substates.swap_remove(pos);
                    // If there was a previous parent for this substate, we keep it as is.
                    let parent = downed_substates_with_parents
                        .get(&owned_id)
                        .cloned()
                        .unwrap_or_else(|| component_addr.clone());

                    if let Some(vault_id) = owned_id.as_vault_id() {
                        if let Some(vault) = tx.vaults_get(&vault_id).optional()? {
                            // The vault for an account may have been mutated without mutating the account component
                            // If we know this vault, set it as a child of the account
                            tx.substates_upsert_child(
                                &vault.account_address.into(),
                                VersionedSubstateIdRef::new(&owned_id, child.version()),
                                [vault.resource_address.into()].into_iter().collect(),
                            )?;
                            if let Some(resource) = tx.substates_get(&vault.resource_address.into()).optional()? {
                                tx.substates_upsert_child(
                                    &vault.account_address.into(),
                                    resource.substate_id.as_versioned_ref(),
                                    HashSet::new(),
                                )?;
                            }
                        } else {
                            tx.substates_upsert_root(
                                VersionedSubstateIdRef::new(&owned_id, child.version()),
                                [(*child.substate_value().vault().unwrap().resource_address()).into()]
                                    .into_iter()
                                    .collect(),
                                None,
                                None,
                            )?;
                        }
                        continue;
                    }

                    let maybe_substate = tx.substates_get(&owned_id).optional()?;
                    tx.substates_upsert_child(
                        &parent,
                        VersionedSubstateIdRef::new(&owned_id, child.version()),
                        maybe_substate
                            .map(|s| s.referenced_substates.into_iter().collect())
                            .unwrap_or_default(),
                    )?;
                }
            }
        }

        for (id, substate) in other_substates {
            match id {
                SubstateId::Component(_) => unreachable!(),
                SubstateId::Resource(_) => match tx.substates_get(id).optional()? {
                    Some(known_substate) => {
                        tx.substates_upsert_root(
                            VersionedSubstateIdRef::new(id, substate.version()),
                            known_substate.referenced_substates.into_iter().collect(),
                            known_substate.module_name,
                            known_substate.template_address,
                        )?;
                    },
                    None => {
                        tx.substates_upsert_root(
                            VersionedSubstateIdRef::new(id, substate.version()),
                            Default::default(),
                            None,
                            None,
                        )?;
                    },
                },
                SubstateId::Vault(vault_id) => {
                    match tx.vaults_get(vault_id).optional()? {
                        Some(vault) => {
                            // The vault for an account may have been mutated without mutating the account component
                            // If we know this vault, set it as a child of the account
                            tx.substates_upsert_child(
                                &vault.account_address.into(),
                                VersionedSubstateIdRef::new(id, substate.version()),
                                [vault.resource_address.into()].into_iter().collect(),
                            )?;
                            if let Some(resource) = tx.substates_get(&vault.resource_address.into()).optional()? {
                                tx.substates_upsert_child(
                                    &vault.account_address.into(),
                                    resource.substate_id.as_versioned_ref(),
                                    HashSet::new(),
                                )?;
                            }
                        },
                        None => {
                            // We don't know the parent account of this vault.
                            debug!(target: LOG_TARGET, "Vault {} does not have a parent", vault_id);
                            // tx.substates_upsert_root(
                            //     VersionedSubstateIdRef::new(id, substate.version()),
                            //     [(*substate
                            //         .substate_value()
                            //         .vault()
                            //         .expect("should be vault")
                            //         .resource_address())
                            //     .into()]
                            //     .into_iter()
                            //     .collect(),
                            //     None,
                            //     None,
                            // )?;
                        },
                    }
                    continue;
                },
                SubstateId::ClaimedOutputTombstone(_) => {
                    tx.substates_upsert_root(
                        VersionedSubstateIdRef::new(id, substate.version()),
                        [TARI_TOKEN.into()].into_iter().collect(),
                        None,
                        None,
                    )?;
                },
                SubstateId::NonFungible(nft) => {
                    let resource_address = nft.resource_address();
                    let referenced_data = substate
                        .substate_value()
                        .non_fungible()
                        .and_then(|s| s.contents())
                        .map(|c| IndexedWellKnownTypes::from_value(c.data()))
                        .transpose()
                        .map_err(TransactionApiError::IndexedValueError)?;
                    let referenced_mdata = substate
                        .substate_value()
                        .non_fungible()
                        .and_then(|s| s.contents())
                        .map(|c| IndexedWellKnownTypes::from_value(c.mutable_data()))
                        .transpose()
                        .map_err(TransactionApiError::IndexedValueError)?;
                    tx.substates_upsert_child(
                        &SubstateId::Resource(*resource_address),
                        VersionedSubstateIdRef::new(id, substate.version()),
                        referenced_data
                            .into_iter()
                            .chain(referenced_mdata)
                            .flat_map(|s| s.into_referenced_substates())
                            .collect(),
                    )?;
                },
                SubstateId::TransactionReceipt(_) | SubstateId::Template(_) | SubstateId::ValidatorFeePool(_) => {
                    tx.substates_upsert_root(
                        VersionedSubstateIdRef::new(id, substate.version()),
                        Default::default(),
                        None,
                        None,
                    )?;
                },
                SubstateId::Utxo(addr) => {
                    tx.substates_upsert_root(
                        VersionedSubstateIdRef::new(id, substate.version()),
                        [(*addr.resource_address()).into()].into_iter().collect(),
                        None,
                        None,
                    )?;
                },
            }
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransactionApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Network interface error: {status} {message}")]
    NetworkInterfaceError {
        status: ResponseErrorStatus,
        message: String,
    },
    #[error("Failed to extract known type data from value: {0}")]
    IndexedValueError(IndexedValueError),
    #[error("Invalid transaction query response: {details}")]
    InvalidTransactionQueryResponse { details: String },
    #[error("Dry run transaction mismatch error: {details}")]
    DryRunMismatchError { details: String },
}

impl IsNotFoundError for TransactionApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::StoreError(e) if e.is_not_found_error() )
    }
}

impl<T: TransactionStatusResponseError> From<T> for TransactionApiError {
    fn from(value: T) -> Self {
        TransactionApiError::NetworkInterfaceError {
            status: value.get_status(),
            message: value.get_error_message(),
        }
    }
}
