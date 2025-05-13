//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
};

use log::*;
use serde::{Deserialize, Serialize};
use tari_dan_common_types::{
    committee::CommitteeInfo,
    displayable::Displayable,
    optional::Optional,
    NumPreshards,
    ToSubstateAddress,
    VersionedSubstateId,
};
use tari_engine_types::transaction_receipt::TransactionReceiptAddress;
use tari_transaction::{Transaction, TransactionId};
use time::PrimitiveDateTime;

use crate::{
    consensus_models::{
        Evidence,
        LockedSubstateValue,
        SubstatePledge,
        SubstatePledges,
        TransactionExecution,
        TransactionPoolRecord,
    },
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};

const LOG_TARGET: &str = "tari::dan::storage::consensus_models::transaction";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub transaction: Transaction,
}

impl TransactionRecord {
    pub fn new(transaction: Transaction) -> Self {
        Self { transaction }
    }

    pub fn id(&self) -> &TransactionId {
        self.transaction.id()
    }

    pub fn transaction(&self) -> &Transaction {
        &self.transaction
    }

    pub fn into_transaction(self) -> Transaction {
        self.transaction
    }

    pub fn is_involved_in_inputs(&self, local_committee_info: &CommitteeInfo) -> bool {
        self.transaction
            .all_inputs_iter()
            .any(|i| local_committee_info.includes_substate_id(i.substate_id()))
    }

    pub fn is_all_local_inputs(&self, local_committee_info: &CommitteeInfo) -> bool {
        self.transaction
            .all_inputs_iter()
            .all(|i| local_committee_info.includes_substate_id(i.substate_id()))
    }

    pub fn to_receipt_id(&self) -> TransactionReceiptAddress {
        (*self.id()).into()
    }

    pub fn to_initial_evidence(&self, num_preshards: NumPreshards, num_committees: u32) -> Evidence {
        let inputs = self.transaction.all_inputs_iter();
        let receipt = self.transaction.id().into_receipt_address();
        Evidence::from_inputs_and_outputs(num_preshards, num_committees, inputs, [VersionedSubstateId::new(
            receipt, 0,
        )])
    }
}

impl TransactionRecord {
    pub fn insert<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.transactions_insert(self)
    }

    pub fn save<TTx>(&self, tx: &mut TTx) -> Result<(), StorageError>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction,
    {
        if !Self::exists(&**tx, self.transaction.id())? {
            self.insert(tx)?;
        }
        Ok(())
    }

    pub fn get<TTx: StateStoreReadTransaction>(tx: &TTx, tx_id: &TransactionId) -> Result<Self, StorageError> {
        tx.transactions_get(tx_id)
    }

    pub fn exists<TTx: StateStoreReadTransaction>(tx: &TTx, tx_id: &TransactionId) -> Result<bool, StorageError> {
        tx.transactions_exists(tx_id)
    }

    pub fn exists_any<'a, TTx: StateStoreReadTransaction, I: IntoIterator<Item = &'a TransactionId>>(
        tx: &TTx,
        tx_ids: I,
    ) -> Result<bool, StorageError> {
        for tx_id in tx_ids {
            if tx.transactions_exists(tx_id)? {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub fn get_any<'a, TTx: StateStoreReadTransaction, I: IntoIterator<Item = &'a TransactionId>>(
        tx: &TTx,
        tx_ids: I,
    ) -> Result<(Vec<Self>, HashSet<TransactionId>), StorageError> {
        let mut tx_ids = tx_ids.into_iter().copied().collect::<HashSet<_>>();
        if tx_ids.is_empty() {
            return Ok((vec![], tx_ids));
        }
        let recs = tx.transactions_get_any(tx_ids.iter())?;
        for rec in &recs {
            tx_ids.remove(rec.transaction.id());
        }

        Ok((recs, tx_ids))
    }

    pub fn get_any_or_build<TTx: StateStoreReadTransaction, I: IntoIterator<Item = Transaction> + Clone>(
        tx: &TTx,
        transactions: I,
    ) -> Result<Vec<Self>, StorageError> {
        let mut tx_ids = transactions
            .clone()
            .into_iter()
            .map(|t| (*t.id(), t))
            .collect::<HashMap<_, _>>();
        let mut recs = tx.transactions_get_any(tx_ids.keys())?;
        for rec in &recs {
            tx_ids.remove(rec.transaction.id());
        }
        recs.extend(tx_ids.into_values().map(Self::new));

        Ok(recs)
    }

    pub fn get_missing<'a, TTx: StateStoreReadTransaction, I: IntoIterator<Item = &'a TransactionId>>(
        tx: &TTx,
        tx_ids: I,
    ) -> Result<HashSet<TransactionId>, StorageError> {
        // TODO(perf): optimise
        let (_, missing) = Self::get_any(tx, tx_ids)?;
        Ok(missing)
    }

    pub fn get_local_pledges<TTx: StateStoreReadTransaction>(&self, tx: &TTx) -> Result<SubstatePledges, StorageError> {
        let locked_values = LockedSubstateValue::get_all_for_transaction(tx, self.id())?;
        locked_values
            .into_iter()
            .filter(|lock| !lock.lock.is_output())
            .map(|mut lock| {
                let maybe_value = lock.take_value();
                let lock_intent = lock.to_substate_lock_intent();
                SubstatePledge::try_create(lock_intent, maybe_value).ok_or_else(|| StorageError::DataInconsistency {
                    details: format!("Invalid substate lock: {} ({})", lock.substate_id, lock.lock),
                })
            })
            .collect()
    }

    pub fn get_finalized_execution<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
    ) -> Result<TransactionExecution, StorageError> {
        tx.finalized_transaction_execution_get(self.id())
    }

    pub fn get_foreign_pledges<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
    ) -> Result<SubstatePledges, StorageError> {
        tx.foreign_substate_pledges_get_all_by_transaction_id(self.id())
    }

    pub fn is_finalized<TTx: StateStoreReadTransaction>(&self, tx: &TTx) -> Result<bool, StorageError> {
        Self::is_record_finalized(tx, self.id())
    }

    pub fn get_finalized_time<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
    ) -> Result<PrimitiveDateTime, StorageError> {
        tx.finalized_transaction_execution_get_finalized_time(self.id())
    }

    pub fn is_record_finalized<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        transaction_id: &TransactionId,
    ) -> Result<bool, StorageError> {
        let time = tx
            .finalized_transaction_execution_get_finalized_time(transaction_id)
            .optional()?;
        Ok(time.is_some())
    }

    pub fn finalize_all<'a, TTx, I>(tx: &mut TTx, transactions: I) -> Result<(), StorageError>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction,
        I: IntoIterator<Item = &'a TransactionPoolRecord>,
    {
        tx.transactions_finalize_all(transactions)
    }

    pub fn has_all_required_input_pledges<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        local_committee_info: &CommitteeInfo,
    ) -> Result<bool, StorageError> {
        let inputs = self
            .transaction()
            .all_inputs_iter()
            .map(|req| (local_committee_info.includes_substate_id(req.substate_id()), req));
        let locks = LockedSubstateValue::get_all_for_transaction(tx, self.id())?;
        let pledges = tx.foreign_substate_pledges_get_all_by_transaction_id(self.id())?;
        for (is_local, input) in inputs {
            if is_local {
                if locks.iter().all(|i| !i.satisfies_requirements(input)) {
                    debug!(
                        target: LOG_TARGET,
                        "Locks: {}",
                        locks.display(),
                    );
                    debug!(
                        target: LOG_TARGET,
                        "{} Transaction {} is missing a local lock for input {} ({} lock(s) found)",
                        local_committee_info.shard_group(),
                        self.id(),
                        input.substate_id(),
                        locks.len(),
                    );
                    return Ok(false);
                }
            } else if pledges.iter().all(|p| !p.satisfies_requirement(input)) {
                let remote_shard_group = input.or_zero_version().to_substate_address().to_shard_group(
                    local_committee_info.num_preshards(),
                    local_committee_info.num_committees(),
                );
                debug!(
                    target: LOG_TARGET,
                    "Pledges: {}",
                    pledges.display(),
                );
                debug!(
                    target: LOG_TARGET,
                    "{} Transaction {} is missing a pledge for input {} from {} ({} pledge(s) found)",
                    local_committee_info.shard_group(),
                    self.id(),
                    input.substate_id(),
                    remote_shard_group,
                    pledges.len(),
                );
                return Ok(false);
            } else {
                // We have a lock/pledge for the input, continue
            }
        }
        Ok(true)
    }

    pub fn has_all_foreign_input_pledges<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        local_committee_info: &CommitteeInfo,
    ) -> Result<bool, StorageError> {
        let mut foreign_inputs = self
            .transaction()
            .all_inputs_iter()
            .filter(|i| !local_committee_info.includes_substate_id(i.substate_id()))
            .peekable();

        if foreign_inputs.peek().is_none() {
            // Avoid query for pledges for no reason
            return Ok(true);
        }

        // TODO(perf): this could be a bespoke DB query
        let pledges = tx.foreign_substate_pledges_get_all_by_transaction_id(self.id())?;
        for input in foreign_inputs {
            if pledges.iter().all(|p| !p.satisfies_requirement(input)) {
                debug!(
                    target: LOG_TARGET,
                    "Transaction {} is missing a pledge for input {} ({} pledge(s) found)",
                    self.id(),
                    input.substate_id(),
                    pledges.len(),
                );
                return Ok(false);
            }
        }
        Ok(true)
    }
}
