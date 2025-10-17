//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::{Deref, DerefMut};

use serde::{de::DeserializeOwned, Serialize};
use tari_consensus_types::BlockId;
use tari_engine_types::{
    events::Event,
    substate::SubstateId,
    transaction_receipt::{TransactionReceipt, TransactionReceiptAddress},
    Utxo,
};
use tari_indexer_client::types::{ListSubstateItem, NonFungibleSubstate, TransactionEntry};
use tari_ootle_common_types::{shard::Shard, substate_type::SubstateType, Epoch, ShardGroup, StateVersion};
use tari_ootle_storage::{
    consensus_models::{EpochCheckpoint, SubstateData, SubstateUpdateProof},
    Ordering,
    StorageError,
};
use tari_ootle_wallet_sdk::models::WalletUtxoUpdate;
use tari_template_lib::{
    models::{ResourceAddress, UtxoId},
    prelude::RistrettoPublicKeyBytes,
    types::{crypto::UtxoTag, TemplateAddress},
};
use tari_transaction::{Transaction, TransactionId};

use crate::{
    network_state_sync::EventFilter,
    storage_sqlite::models::{KeyValue, NewScannedBlockId, SubstateRecord, UtxoUpdateRecord},
    substate_manager::SubstateResponse,
};

const LOG_TARGET: &str = "tari::indexer::store";

pub trait IndexerStore: IndexerStoreReader {
    type WriteTransaction<'a>: IndexerStoreWriteTransaction + Deref<Target = Self::ReadTransaction<'a>> + DerefMut
    where Self: 'a;

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, StorageError>;

    fn with_write_tx<F: FnOnce(&mut Self::WriteTransaction<'_>) -> Result<R, E>, R, E>(&self, f: F) -> Result<R, E>
    where E: From<StorageError> {
        let mut tx = self.create_write_tx()?;
        match f(&mut tx) {
            Ok(r) => {
                tx.commit()?;
                Ok(r)
            },
            Err(e) => {
                if let Err(err) = tx.rollback() {
                    log::error!(target: LOG_TARGET, "Failed to rollback transaction: {}", err);
                }
                Err(e)
            },
        }
    }
}

pub trait IndexerStoreReader {
    type ReadTransaction<'a>: IndexerStoreReadTransaction
    where Self: 'a;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, StorageError>;

    fn with_read_tx<F: FnOnce(&mut Self::ReadTransaction<'_>) -> Result<R, E>, R, E>(&self, f: F) -> Result<R, E>
    where E: From<StorageError> {
        let mut tx = self.create_read_tx()?;
        let ret = f(&mut tx)?;
        Ok(ret)
    }
}

pub trait IndexerStoreReadTransaction {
    fn list_substates(
        &mut self,
        filter_by_type: Option<SubstateType>,
        filter_by_template: Option<TemplateAddress>,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<ListSubstateItem>, StorageError>;
    fn get_substate(
        &mut self,
        address: &SubstateId,
        version: Option<u32>,
    ) -> Result<Option<SubstateRecord>, StorageError>;

    fn get_substates(&mut self, ids: &[SubstateId]) -> Result<Vec<SubstateResponse>, StorageError>;
    fn get_non_fungible_count(&mut self, resource_address: String) -> Result<i64, StorageError>;
    fn get_non_fungibles_by_resource_address(
        &mut self,
        resource_address: ResourceAddress,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<NonFungibleSubstate>, StorageError>;

    fn get_events(
        &mut self,
        substate_id_filter: Option<&SubstateId>,
        topic_filter: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<(TransactionId, Event)>, StorageError>;
    fn get_oldest_scanned_epoch(&mut self) -> Result<Option<Epoch>, StorageError>;
    fn get_last_scanned_block_id(
        &mut self,
        epoch: Epoch,
        shard_group: ShardGroup,
    ) -> Result<Option<BlockId>, StorageError>;
    fn list_recent_transactions(
        &mut self,
        last_transaction_id: Option<TransactionId>,
        limit: usize,
    ) -> Result<Vec<TransactionEntry>, StorageError>;

    // -------------------------------- Transaction Receipts -------------------------------- //
    fn list_transaction_receipts(
        &mut self,
        last_id: Option<TransactionReceiptAddress>,
        limit: u64,
        ordering: Ordering,
    ) -> Result<Vec<(TransactionReceiptAddress, TransactionReceipt)>, StorageError>;

    fn get_transaction_receipt(
        &mut self,
        address: &TransactionReceiptAddress,
    ) -> Result<TransactionReceipt, StorageError>;

    // -------------------------------- KeyValues -------------------------------- //
    fn key_value_get_value<K: AsRef<str>, T: DeserializeOwned>(&mut self, key: K) -> Result<T, StorageError>;
    fn key_value_get_raw<K: AsRef<str>>(&mut self, key: K) -> Result<KeyValue<String>, StorageError>;

    fn utxos_get_max_state_version(
        &mut self,
        resource_address: ResourceAddress,
        shard: Shard,
    ) -> Result<StateVersion, StorageError>;

    /// Get UTXO updates for a given resource address and shard, starting from a specific state version.
    ///
    /// Returns a tuple containing the maximum returned state version, and a vector of UTXO
    /// updates.
    fn utxos_get_updates(
        &mut self,
        resource_address: ResourceAddress,
        shard: Shard,
        from_state_version: StateVersion,
        unspents_only: bool,
        limit: u32,
    ) -> Result<(StateVersion, Vec<WalletUtxoUpdate>), StorageError>;

    fn utxos_get_unspent_by_public_nonce_and_tag(
        &mut self,
        resource_address: &ResourceAddress,
        public_nonce_and_tag: &[(UtxoTag, RistrettoPublicKeyBytes)],
    ) -> Result<Vec<(UtxoId, Utxo)>, StorageError>;
}

pub trait IndexerStoreWriteTransaction {
    fn commit(self) -> Result<(), StorageError>;
    fn rollback(self) -> Result<(), StorageError>;
    fn key_value_set<K: AsRef<str>, V: Serialize>(&mut self, key: K, value: V) -> Result<(), StorageError>;
    fn batch_insert_substate_transitions<I: IntoIterator<Item = (Epoch, SubstateUpdateProof)>>(
        &mut self,
        shard: Shard,
        state_version: StateVersion,
        updates: I,
    ) -> Result<(), StorageError>;
    fn batch_insert_utxo_updates<I: IntoIterator<Item = UtxoUpdateRecord>>(
        &mut self,
        updates: I,
    ) -> Result<(), StorageError>;
    fn upsert_substate(&mut self, substate: &SubstateData) -> Result<(), StorageError>;
    fn batch_insert_transaction_receipts<I: IntoIterator<Item = (TransactionReceiptAddress, TransactionReceipt)>>(
        &mut self,
        receipts: I,
        event_filters: &[EventFilter],
    ) -> Result<(), StorageError>;
    fn save_scanned_block_id(&mut self, new_scanned_block_id: NewScannedBlockId) -> Result<(), StorageError>;
    fn delete_scanned_epochs_older_than(&mut self, epoch: Epoch) -> Result<(), StorageError>;
    fn insert_or_ignore_transaction(&mut self, transaction: &Transaction) -> Result<(), StorageError>;
    fn insert_or_ignore_epoch_checkpoint(&mut self, epoch_checkpoint: &EpochCheckpoint) -> Result<(), StorageError>;
}

pub struct ReadOnlyStore<T: IndexerStoreReader> {
    inner: T,
}

impl<T: IndexerStoreReader> ReadOnlyStore<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn list_transaction_receipts(
        &self,
        last_id: Option<TransactionReceiptAddress>,
        limit: u64,
        ordering: Ordering,
    ) -> Result<Vec<(TransactionReceiptAddress, TransactionReceipt)>, StorageError> {
        self.inner
            .with_read_tx(|tx| tx.list_transaction_receipts(last_id, limit, ordering))
    }

    pub fn get_transaction_receipt(
        &self,
        address: &TransactionReceiptAddress,
    ) -> Result<TransactionReceipt, StorageError> {
        self.inner.with_read_tx(|tx| tx.get_transaction_receipt(address))
    }
}
