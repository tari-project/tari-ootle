//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use diesel::{OptionalExtension, QueryDsl, RunQueryDsl, SqliteConnection};
use log::{debug, info, warn};
use serde::Serialize;
use tari_engine_types::{
    published_template::PublishedTemplateMetadata,
    substate::SubstateId,
    transaction_receipt::TransactionReceipt,
};
use tari_ootle_common_types::{Epoch, StateVersion, shard::Shard, substate_type::SubstateType};
use tari_ootle_storage::{
    StorageError,
    consensus_models::{EpochCheckpoint, SubstateData, SubstateUpdateProof},
};
use tari_ootle_storage_sqlite::SqliteTransaction;
use tari_ootle_transaction::{Transaction, TransactionId};
use tari_template_lib_types::{TemplateAddress, TransactionReceiptAddress};

use crate::{
    diesel::ExpressionMethods,
    network_state_sync::EventFilter,
    storage_sqlite::{
        models::{
            NewEvent,
            NewSubstate,
            NewTemplateCatalogueRow,
            NewWatchedSubstate,
            SubstateRecord,
            UtxoRecordInsert,
            UtxoRecordUpdate,
            UtxoUpdateRecord,
        },
        reader::SqliteStoreReadTransaction,
        serialization::{serialize_bincode, serialize_hex, serialize_json},
        store_factory::SqlitePooledConnection,
    },
    store::{IndexerStoreWriteTransaction, InsertedEvent},
};

const LOG_TARGET: &str = "tari::indexer::storage_sqlite::writer";

pub struct SqliteStoreWriteTransaction {
    /// None indicates if the transaction has been explicitly committed/rolled back
    transaction: Option<SqliteStoreReadTransaction>,
}

impl SqliteStoreWriteTransaction {
    pub fn new(transaction: SqliteTransaction<SqlitePooledConnection>) -> Self {
        Self {
            transaction: Some(SqliteStoreReadTransaction::new(transaction)),
        }
    }

    fn connection(&mut self) -> &mut SqliteConnection {
        self.transaction.as_mut().unwrap().connection()
    }
}
impl IndexerStoreWriteTransaction for SqliteStoreWriteTransaction {
    fn commit(mut self) -> Result<(), StorageError> {
        self.transaction.take().unwrap().transaction.commit()?;
        Ok(())
    }

    fn rollback(mut self) -> Result<(), StorageError> {
        self.transaction.take().unwrap().transaction.rollback()?;
        Ok(())
    }

    fn key_value_set<K: AsRef<str>, V: Serialize>(&mut self, key: K, value: V) -> Result<(), StorageError> {
        const OPERATION: &str = "key_value_set";
        use crate::storage_sqlite::schema::key_values;
        let json = serialize_json(&value)?;
        debug!(target: LOG_TARGET, "key_value_set called {} {}", key.as_ref(), json);

        diesel::insert_into(key_values::table)
            .values((key_values::key.eq(key.as_ref()), key_values::value.eq(&json)))
            .on_conflict(key_values::key)
            .do_update()
            .set(key_values::value.eq(&json))
            .execute(self.connection())
            .map_err(|e| StorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn batch_insert_substate_transitions<I: IntoIterator<Item = (Epoch, SubstateUpdateProof)>>(
        &mut self,
        shard: Shard,
        state_version: StateVersion,
        updates: I,
    ) -> Result<(), StorageError> {
        const OPERATION: &str = "batch_insert_substate_transitions";
        use crate::storage_sqlite::schema::substate_transitions;

        diesel::insert_into(substate_transitions::table)
            .values(
                updates
                    .into_iter()
                    .map(|(epoch, proof)| {
                        (
                            substate_transitions::shard.eq(shard.as_u32() as i32),
                            substate_transitions::state_version.eq(state_version.as_u64() as i64),
                            substate_transitions::epoch.eq(epoch.as_u64() as i64),
                            substate_transitions::substate_id.eq(proof.substate_id().to_string()),
                            substate_transitions::substate_type.eq(SubstateType::from(proof.substate_id()).to_string()),
                            substate_transitions::version.eq(proof.version() as i32),
                            substate_transitions::is_up.eq(proof.is_create()),
                            substate_transitions::value_hash.eq(proof
                                .as_create()
                                .map(|v| serialize_hex(v.substate.value.to_value_hash(proof.version(), epoch)))),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .execute(self.connection())
            .map_err(|e| StorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn batch_insert_utxo_updates<I: IntoIterator<Item = UtxoUpdateRecord>>(
        &mut self,
        epoch: Epoch,
        updates: I,
    ) -> Result<(), StorageError> {
        const OPERATION: &str = "batch_insert_utxo_updates";
        use crate::storage_sqlite::schema::utxos;

        for update in updates {
            match update {
                UtxoUpdateRecord::Unspent(unspent) => {
                    let resource_address = unspent.address.resource_address().to_string();
                    let commitment = unspent.address.id().to_commitment_hex_string();

                    let insert = UtxoRecordInsert {
                        commitment,
                        public_nonce: serialize_hex(unspent.utxo_output.output.public_nonce),
                        version: unspent.version as i32,
                        output: Some(serialize_bincode(&unspent.utxo_output)?),
                        shard: unspent.shard.as_u32() as i32,
                        resource_address,
                        state_version: unspent.state_version.as_u64() as i64,
                        utxo_tag: unspent.utxo_output.tag.value() as i32,
                        epoch: epoch.as_u64() as i64,
                        is_spent: false,
                        is_burnt: false,
                        is_frozen: unspent.is_frozen,
                    };
                    // batch insert results in "cannot start a transaction within a transaction" error
                    diesel::insert_into(utxos::table)
                        .values(insert)
                        .execute(self.connection())
                        .map_err(|e| StorageError::general(OPERATION, format!("insert error: {e}")))?;
                },
                UtxoUpdateRecord::Spent(spent) => {
                    let resource_address = spent.address.resource_address().to_string();
                    let commitment = spent.address.id().to_commitment_hex_string();
                    let update = UtxoRecordUpdate {
                        epoch: Some(epoch.as_u64() as i64),
                        version: Some(spent.version as i32),
                        // Prune the UTXO data for spent outputs
                        output: Some(None),
                        // Update to deleted state version
                        state_version: Some(spent.state_version.as_u64() as i64),
                        is_spent: Some(true),
                        ..Default::default()
                    };
                    diesel::update(utxos::table)
                        .filter(utxos::resource_address.eq(resource_address))
                        .filter(utxos::commitment.eq(commitment))
                        .set(update)
                        .execute(self.connection())
                        .map_err(|e| StorageError::general(OPERATION, format!("update error: {e}")))?;
                },
            }
        }

        Ok(())
    }

    fn upsert_substate(&mut self, substate: &SubstateData) -> Result<(), StorageError> {
        use crate::storage_sqlite::schema::substates;

        let template_address = substate
            .value
            .value()
            .and_then(|s| s.component())
            .map(|c| c.template_address().to_string());
        let new_substate = NewSubstate {
            address: substate.substate_id.to_string(),
            version: substate.version as i32,
            data: substate
                .value
                .value()
                .map(serialize_json)
                .transpose()?
                .unwrap_or_default(),
            template_address,
            // Never set
            module_name: None,
        };

        let address = &new_substate.address;
        let current_substate = substates::table
            .filter(substates::address.eq(address))
            .first::<SubstateRecord>(self.connection())
            .optional()
            .map_err(|e| StorageError::QueryError {
                reason: format!("find_by_address: {}", e),
            })?;

        match current_substate {
            Some(_) => {
                diesel::update(substates::table)
                    .set(&new_substate)
                    .filter(substates::address.eq(address))
                    .execute(self.connection())
                    .map_err(|e| StorageError::QueryError {
                        reason: format!("Update leaf node: {}", e),
                    })?;
                debug!(
                    target: LOG_TARGET,
                    "Updated substate {} version to {}", address, new_substate.version
                );
            },
            None => {
                diesel::insert_into(substates::table)
                    .values(&new_substate)
                    .execute(self.connection())
                    .map_err(|e| StorageError::QueryError {
                        reason: format!("Update substate error: {}", e),
                    })?;
                info!(
                    target: LOG_TARGET,
                    "Added new substate {} with version {}", address, new_substate.version
                );
            },
        };

        Ok(())
    }

    fn batch_insert_transaction_receipts<I: IntoIterator<Item = (TransactionReceiptAddress, TransactionReceipt)>>(
        &mut self,
        receipts: I,
        event_filters: &[EventFilter],
    ) -> Result<Vec<InsertedEvent>, StorageError> {
        const OPERATION: &str = "batch_insert_transaction_receipts";
        use crate::storage_sqlite::schema::{events, transaction_receipts};

        let mut inserted_events = Vec::new();

        for (receipt_addr, receipt) in receipts {
            let receipt_addr_hex = serialize_hex(receipt_addr.as_object_key());
            let transaction_id = TransactionId::from_receipt_address(receipt_addr);

            diesel::insert_into(transaction_receipts::table)
                .values((
                    transaction_receipts::address.eq(&receipt_addr_hex),
                    transaction_receipts::data.eq(serialize_json(&receipt)?),
                ))
                .execute(self.connection())
                .map_err(|e| StorageError::general(OPERATION, e))?;

            // Insert events and collect assigned IDs
            let filtered_events: Vec<_> = receipt
                .events
                .iter()
                .filter(|event| event_filters.is_empty() || event_filters.iter().any(|filter| filter.matches(event)))
                .cloned()
                .collect();

            for event in filtered_events {
                let new_event = NewEvent {
                    template_address: event.template_address().to_string(),
                    tx_hash: &receipt_addr_hex,
                    topic: event.topic(),
                    payload: serialize_json(event.payload())?,
                    substate_id: event.substate_id().map(|s| s.to_string()),
                    resource_address: EventFilter::event_resource_address(&event).map(|r| r.to_string()),
                };

                let id: i64 = diesel::insert_into(events::table)
                    .values(new_event)
                    .returning(events::id)
                    .get_result(self.connection())
                    .map_err(|e| StorageError::QueryError {
                        reason: format!("{OPERATION}: {}", e),
                    })?;

                inserted_events.push(InsertedEvent {
                    id,
                    transaction_id,
                    event: Arc::new(event),
                });
            }
        }

        Ok(inserted_events)
    }

    fn insert_or_ignore_transaction(&mut self, transaction: &Transaction) -> Result<(), StorageError> {
        use crate::storage_sqlite::schema::transactions;

        diesel::insert_into(transactions::table)
            .values((
                transactions::transaction_id.eq(serialize_hex(transaction.calculate_id())),
                transactions::body.eq(serialize_json(transaction).unwrap()),
            ))
            .on_conflict_do_nothing()
            .execute(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("insert_transaction: {e}"),
            })?;

        Ok(())
    }

    fn insert_or_ignore_epoch_checkpoint(&mut self, epoch_checkpoint: &EpochCheckpoint) -> Result<(), StorageError> {
        const OPERATION: &str = "insert_or_ignore_epoch_checkpoint";
        use crate::storage_sqlite::schema::epoch_checkpoints;

        diesel::insert_into(epoch_checkpoints::table)
            .values((
                epoch_checkpoints::epoch.eq(epoch_checkpoint.epoch().as_u64() as i64),
                epoch_checkpoints::shard_group.eq(epoch_checkpoint
                    .checked_shard_group()
                    .expect("shard group should be valid")
                    .to_parsable_string()),
                epoch_checkpoints::json_data.eq(serialize_json(epoch_checkpoint)?),
            ))
            .on_conflict((epoch_checkpoints::epoch, epoch_checkpoints::shard_group))
            .do_nothing()
            .execute(self.connection())
            .map_err(|e| StorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn upsert_template_catalogue(
        &mut self,
        template_address: &TemplateAddress,
        metadata: &PublishedTemplateMetadata,
    ) -> Result<(), StorageError> {
        const OPERATION: &str = "upsert_template_catalogue";
        use crate::storage_sqlite::schema::template_catalogue;

        let row = NewTemplateCatalogueRow::from((*template_address, metadata));

        diesel::insert_into(template_catalogue::table)
            .values(&row)
            .on_conflict(template_catalogue::template_address)
            .do_update()
            .set(&row)
            .execute(self.connection())
            .map_err(|e| StorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn insert_watched_substate(
        &mut self,
        component_address: &SubstateId,
        template_address: &TemplateAddress,
    ) -> Result<(), StorageError> {
        const OPERATION: &str = "insert_watched_substate";
        use crate::storage_sqlite::schema::watched_substates;

        let component_addr_str = component_address.to_string();
        let template_addr_str = template_address.to_string();

        diesel::insert_into(watched_substates::table)
            .values(NewWatchedSubstate {
                component_address: &component_addr_str,
                template_address: &template_addr_str,
            })
            .on_conflict(watched_substates::component_address)
            .do_update()
            .set(watched_substates::template_address.eq(&template_addr_str))
            .execute(self.connection())
            .map_err(|e| StorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn delete_watched_substate(&mut self, component_address: &SubstateId) -> Result<(), StorageError> {
        const OPERATION: &str = "delete_watched_substate";
        use crate::storage_sqlite::schema::watched_substates;

        diesel::delete(
            watched_substates::table.filter(watched_substates::component_address.eq(component_address.to_string())),
        )
        .execute(self.connection())
        .map_err(|e| StorageError::general(OPERATION, e))?;

        Ok(())
    }
}

impl Deref for SqliteStoreWriteTransaction {
    type Target = SqliteStoreReadTransaction;

    fn deref(&self) -> &Self::Target {
        self.transaction.as_ref().unwrap()
    }
}

impl DerefMut for SqliteStoreWriteTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.transaction.as_mut().unwrap()
    }
}

impl Drop for SqliteStoreWriteTransaction {
    fn drop(&mut self) {
        if self.transaction.is_some() {
            warn!(
                target: LOG_TARGET,
                "Substate store write transaction was not committed/rolled back"
            );
        }
    }
}
