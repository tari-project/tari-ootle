//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{
    collections::HashSet,
    fmt::Debug,
    fs::create_dir_all,
    ops::{Deref, DerefMut},
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};

use diesel::{prelude::*, sql_query, SqliteConnection};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness};
use log::*;
use serde::{de::DeserializeOwned, Serialize};
use tari_consensus_types::BlockId;
use tari_crypto::tari_utilities::hex::to_hex;
use tari_engine_types::{
    events::Event,
    substate::{SubstateId, SubstateValue},
};
use tari_indexer_client::types::{ListSubstateItem, TransactionEntry};
use tari_indexer_lib::NonFungibleSubstate;
use tari_ootle_common_types::{shard::Shard, substate_type::SubstateType, Epoch, ShardGroup, StateVersion, UtxoUpdate};
use tari_ootle_storage::{
    consensus_models::{EpochCheckpoint, SubstateUpdateProof},
    time::PrimitiveDateTime,
    StorageError,
};
use tari_ootle_storage_sqlite::{error::SqliteStorageError, SqliteTransaction};
use tari_template_lib::{
    models::ResourceAddress,
    types::{crypto::UtxoTagByte, TemplateAddress},
};
use tari_transaction::{Transaction, TransactionId};

use super::models::{NewEvent, NewScannedBlockId, UtxoRecordInsert, UtxoRecordUpdate};
use crate::storage_sqlite::{
    models,
    models::{EventDb, KeyValue, NewSubstate, ScannedBlockId, Substate},
    schema,
    serialization::{deserialize_hex_try_from, deserialize_json, serialize_hex, serialize_json},
};

const LOG_TARGET: &str = "tari::indexer::storage_sqlite";

#[derive(Clone)]
pub struct SqliteIndexerStore {
    connection: Arc<Mutex<SqliteConnection>>,
}

impl SqliteIndexerStore {
    pub fn try_create(path: PathBuf) -> Result<Self, StorageError> {
        create_dir_all(path.parent().unwrap()).map_err(|_| StorageError::FileSystemPathDoesNotExist)?;

        let database_url = path.to_str().expect("database_url utf-8 error").to_string();
        let mut connection = SqliteConnection::establish(&database_url).map_err(SqliteStorageError::from)?;

        pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./src/storage_sqlite/migrations");
        if let Err(err) = connection.run_pending_migrations(MIGRATIONS) {
            log::error!(target: LOG_TARGET, "Error running migrations: {}", err);
        }
        sql_query("PRAGMA foreign_keys = ON;")
            .execute(&mut connection)
            .map_err(|source| SqliteStorageError::DieselError {
                source,
                operation: "set pragma".to_string(),
            })?;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }
}
impl Debug for SqliteIndexerStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SqliteIndexerStore {{ connection: ... }}")
    }
}
pub trait IndexerStore {
    type ReadTransaction<'a>: IndexerStoreReadTransaction
    where Self: 'a;
    type WriteTransaction<'a>: IndexerStoreWriteTransaction + Deref<Target = Self::ReadTransaction<'a>> + DerefMut
    where Self: 'a;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, StorageError>;
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

    fn with_read_tx<F: FnOnce(&mut Self::ReadTransaction<'_>) -> Result<R, E>, R, E>(&self, f: F) -> Result<R, E>
    where E: From<StorageError> {
        let mut tx = self.create_read_tx()?;
        let ret = f(&mut tx)?;
        Ok(ret)
    }
}

impl IndexerStore for SqliteIndexerStore {
    type ReadTransaction<'a> = SqliteStoreReadTransaction<'a>;
    type WriteTransaction<'a> = SqliteStoreWriteTransaction<'a>;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, StorageError> {
        let tx = SqliteTransaction::begin(self.connection.lock().unwrap())?;
        Ok(SqliteStoreReadTransaction::new(tx))
    }

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, StorageError> {
        let tx = SqliteTransaction::begin(self.connection.lock().unwrap())?;
        Ok(SqliteStoreWriteTransaction::new(tx))
    }
}

pub struct SqliteStoreReadTransaction<'a> {
    transaction: SqliteTransaction<'a>,
}

impl<'a> SqliteStoreReadTransaction<'a> {
    fn new(transaction: SqliteTransaction<'a>) -> Self {
        Self { transaction }
    }

    fn connection(&mut self) -> &mut SqliteConnection {
        self.transaction.connection()
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
    fn get_substate(&mut self, address: &SubstateId, version: Option<u32>) -> Result<Option<Substate>, StorageError>;
    fn get_non_fungible_count(&mut self, resource_address: String) -> Result<i64, StorageError>;
    fn get_non_fungibles_by_resource_address(
        &mut self,
        resource_address: ResourceAddress,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<NonFungibleSubstate>, StorageError>;

    fn get_events(
        &mut self,
        substate_id_filter: Option<SubstateId>,
        topic_filter: Option<String>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<EventDb>, StorageError>;
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

    // -------------------------------- KeyValues -------------------------------- //
    fn key_value_get_value<K: AsRef<str>, T: DeserializeOwned>(&mut self, key: K) -> Result<T, StorageError>;
    fn key_value_get_raw<K: AsRef<str>>(&mut self, key: K) -> Result<KeyValue<String>, StorageError>;

    /// Get UTXO updates for a given resource address and shard, starting from a specific state version.
    ///
    /// Returns a tuple containing the total count of matching UTXO updates (without the limit) and a vector of UTXO
    /// updates limited by the specified limit.
    fn get_utxo_updates(
        &mut self,
        resource_address: ResourceAddress,
        shard: Shard,
        from_state_version: StateVersion,
        filter_tag_bytes: &HashSet<UtxoTagByte>,
        limit: u32,
    ) -> Result<Vec<UtxoUpdate>, StorageError>;
}

impl IndexerStoreReadTransaction for SqliteStoreReadTransaction<'_> {
    fn list_substates(
        &mut self,
        by_type: Option<SubstateType>,
        by_template_address: Option<TemplateAddress>,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<ListSubstateItem>, StorageError> {
        use crate::storage_sqlite::schema::substates;

        let mut query = substates::table.into_boxed();

        if let Some(template_address) = by_template_address {
            query = query.filter(substates::template_address.eq(template_address.to_string()));
        }

        if let Some(substate_type) = by_type {
            let address_like = format!("{}_%", substate_type.as_prefix_str());
            query = query.filter(substates::address.like(address_like));
        }

        if let Some(limit) = limit {
            query = query.limit(limit as i64);
        }
        if let Some(offset) = offset {
            query = query.offset(offset as i64);
        }

        let substates: Vec<Substate> = query
            .order_by(substates::id.desc())
            .get_results(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("list_substates: {}", e),
            })?;

        let items = substates
            .into_iter()
            .map(|s| {
                let substate_id = SubstateId::from_str(&s.address)?;
                let version = s.version as u32;
                let template_address = s.template_address.map(|h| deserialize_hex_try_from(&h)).transpose()?;
                let timestamp = s.timestamp;
                Ok(ListSubstateItem {
                    substate_id,
                    module_name: s.module_name,
                    version,
                    template_address,
                    timestamp,
                })
            })
            .collect::<Result<Vec<ListSubstateItem>, anyhow::Error>>()
            .map_err(|e| StorageError::QueryError {
                reason: format!("list_substates: invalid substate items: {}", e),
            })?;

        Ok(items)
    }

    fn get_substate(&mut self, address: &SubstateId, version: Option<u32>) -> Result<Option<Substate>, StorageError> {
        use crate::storage_sqlite::schema::substates;

        let mut substate_query = substates::table
            .into_boxed()
            .filter(substates::address.eq(address.to_string()));
        if let Some(version) = version {
            substate_query = substate_query.filter(substates::version.eq(version as i32));
        } else {
            substate_query = substate_query.order_by(substates::version.desc())
        }

        substate_query
            .limit(1)
            .first(self.connection())
            .optional()
            .map_err(|e| StorageError::QueryError {
                reason: format!("get_substate: {}", e),
            })
    }

    fn get_non_fungible_count(&mut self, resource_address: String) -> Result<i64, StorageError> {
        use crate::storage_sqlite::schema::non_fungible_indexes;

        let count = non_fungible_indexes::table
            .filter(non_fungible_indexes::resource_address.eq(resource_address))
            .count()
            .get_result::<i64>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("get_non_fungible_count: {}", e),
            })?;

        Ok(count)
    }

    fn get_non_fungibles_by_resource_address(
        &mut self,
        resource_address: ResourceAddress,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<NonFungibleSubstate>, StorageError> {
        use crate::storage_sqlite::schema::substates;

        let res = substates::table
            .filter(substates::address.like(format!("nft_{}_%", resource_address.as_object_key())))
            .limit(limit as i64)
            .offset(offset as i64)
            .get_results::<Substate>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("get_non_fungibles_by_resource_address: {}", e),
            })?;

        res.into_iter()
            .map(|row| {
                let value: SubstateValue =
                    serde_json::from_str(&row.data).map_err(|e| StorageError::DataInconsistency {
                        details: format!("Failed to parse substate data: {}", e),
                    })?;

                Ok(NonFungibleSubstate {
                    address: row.address.parse().map_err(|e| StorageError::DataInconsistency {
                        details: format!("Failed to parse address: {}", e),
                    })?,
                    version: row.version.try_into().map_err(|e| StorageError::DataInconsistency {
                        details: format!("Version overflow {}", e),
                    })?,
                    substate: value,
                })
            })
            .collect()
    }

    fn get_events(
        &mut self,
        substate_id_filter: Option<SubstateId>,
        topic_filter: Option<String>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<EventDb>, StorageError> {
        // TODO: allow to query by payload as well, unifying all event methods into one
        info!(
            target: LOG_TARGET,
            "Querying substate scanner database: get_events with substate_id_filter = {:?} and \
            topic_filter = {:?}",
            substate_id_filter,
            topic_filter
        );
        use crate::storage_sqlite::schema::events;

        let mut query = events::table.into_boxed();

        if let Some(substate_id) = substate_id_filter {
            query = query.filter(events::substate_id.eq(substate_id.to_string()));
        }

        if let Some(topic) = topic_filter {
            query = query.filter(events::topic.eq(topic));
        }

        query = query.offset(offset.into());
        if limit > 0 {
            query = query.limit(limit.into());
        }

        let events = query
            .order(events::id.desc())
            .get_results::<EventDb>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("get_events: {}", e),
            })?;

        Ok(events)
    }

    fn get_oldest_scanned_epoch(&mut self) -> Result<Option<Epoch>, StorageError> {
        use crate::storage_sqlite::schema::scanned_block_ids;

        let res: Option<i64> = scanned_block_ids::table
            .select(diesel::dsl::min(scanned_block_ids::epoch))
            .first(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("get_oldest_scanned_epoch: {}", e),
            })?;

        let oldest_epoch = res
            .map(|r| {
                let epoch_as_u64 = r as u64;
                Ok::<Epoch, StorageError>(Epoch(epoch_as_u64))
            })
            .transpose()?;

        Ok(oldest_epoch)
    }

    fn get_last_scanned_block_id(
        &mut self,
        epoch: Epoch,
        shard_group: ShardGroup,
    ) -> Result<Option<BlockId>, StorageError> {
        use crate::storage_sqlite::schema::scanned_block_ids;

        let row: Option<ScannedBlockId> = scanned_block_ids::table
            .filter(
                scanned_block_ids::epoch
                    .eq(epoch.0 as i64)
                    .and(scanned_block_ids::shard_group.eq(shard_group.encode_as_u32() as i32)),
            )
            .first(self.connection())
            .optional()
            .map_err(|e| StorageError::QueryError {
                reason: format!("get_last_scanned_block_id: {}", e),
            })?;

        let block_id_option = row.map(|r| BlockId::try_from(r.last_block_id)).transpose()?;

        Ok(block_id_option)
    }

    fn list_recent_transactions(
        &mut self,
        last_transaction_id: Option<TransactionId>,
        limit: usize,
    ) -> Result<Vec<TransactionEntry>, StorageError> {
        use crate::storage_sqlite::schema::transactions;

        let start_id = if let Some(last_id) = last_transaction_id {
            transactions::table
                .select(transactions::id)
                .filter(transactions::transaction_id.eq(serialize_hex(last_id)))
                .first::<i32>(self.connection())
                .map_err(|e| StorageError::QueryError {
                    reason: format!("list_recent_transactions: {e}"),
                })?
        } else {
            i32::MAX
        };

        let rows = transactions::table
            .select((transactions::body, transactions::created_at))
            .filter(transactions::id.lt(start_id))
            .order_by(transactions::id.desc())
            .limit(limit as i64)
            .load_iter(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("get_last_scanned_block_id: {}", e),
            })?;

        rows.map(|r| {
            r.map_err(|e| StorageError::QueryError {
                reason: format!("get_last_scanned_block_id: {}", e),
            })
            .and_then(|(body, created_at): (String, PrimitiveDateTime)| {
                let transaction: Transaction = deserialize_json(&body)?;
                Ok(TransactionEntry {
                    transaction_id: transaction.calculate_id(),
                    created_at,
                    transaction,
                })
            })
        })
        .collect()
    }

    // -------------------------------- KeyValues -------------------------------- //
    fn key_value_get_value<K: AsRef<str>, T: DeserializeOwned>(&mut self, key: K) -> Result<T, StorageError> {
        let key_value = self.key_value_get_raw(key)?;
        deserialize_json(&key_value.value)
    }

    fn key_value_get_raw<K: AsRef<str>>(&mut self, key: K) -> Result<KeyValue<String>, StorageError> {
        const OPERATION: &str = "key_value_get_json";
        use schema::key_values;

        let key_value = key_values::table
            .filter(key_values::key.eq(key.as_ref()))
            .first::<models::KeyValueEntry>(self.connection())
            .optional()
            .map_err(|e| StorageError::general(OPERATION, e))?
            .ok_or_else(|| StorageError::NotFound {
                item: "key_value",
                key: key.as_ref().to_string(),
            })?;

        Ok(key_value.into())
    }

    fn get_utxo_updates(
        &mut self,
        resource_address: ResourceAddress,
        shard: Shard,
        from_state_version: StateVersion,
        filter_tag_bytes: &HashSet<UtxoTagByte>,
        limit: u32,
    ) -> Result<Vec<UtxoUpdate>, StorageError> {
        const OPERATION: &str = "get_utxo_updates";
        use crate::storage_sqlite::schema::utxos;

        let mut query = utxos::table
            .filter(utxos::resource_address.eq(resource_address.to_string()))
            .filter(utxos::state_version.gt(from_state_version.as_u64() as i64))
            .filter(utxos::shard.eq(shard.as_u32() as i32))
            .into_boxed();

        if !filter_tag_bytes.is_empty() {
            query = query.filter(utxos::utxo_tag_byte.eq_any(filter_tag_bytes.iter().map(|b| i32::from(b.as_byte()))));
        }

        let rows = query
            .limit(i64::from(limit))
            .order_by(utxos::state_version.asc())
            .load_iter::<models::UtxoRecord, _>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("{OPERATION}: {}", e),
            })?;

        // NOTE: I doubt the size hint > 0
        let (lower, upper) = rows.size_hint();
        let size_hint = upper.unwrap_or(lower);
        let mut updates = Vec::with_capacity(size_hint);
        for row in rows {
            let row = row.map_err(|e| StorageError::QueryError {
                reason: format!("{OPERATION}: {}", e),
            })?;

            updates.push(row.try_convert()?);
        }

        Ok(updates)
    }
}

pub struct SqliteStoreWriteTransaction<'a> {
    /// None indicates if the transaction has been explicitly committed/rolled back
    transaction: Option<SqliteStoreReadTransaction<'a>>,
}

impl<'a> SqliteStoreWriteTransaction<'a> {
    pub fn new(transaction: SqliteTransaction<'a>) -> Self {
        Self {
            transaction: Some(SqliteStoreReadTransaction::new(transaction)),
        }
    }

    pub fn connection(&mut self) -> &mut SqliteConnection {
        self.transaction.as_mut().unwrap().connection()
    }
}

// TODO: remove the allow dead_code attributes as these become used.
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
    fn batch_insert_utxo_updates<I: IntoIterator<Item = UtxoUpdate>>(&mut self, updates: I)
        -> Result<(), StorageError>;
    fn upsert_substate(&mut self, new_substate: NewSubstate) -> Result<(), StorageError>;
    fn batch_insert_events<I: IntoIterator<Item = Event>>(&mut self, events: I) -> Result<(), StorageError>;
    fn save_scanned_block_id(&mut self, new_scanned_block_id: NewScannedBlockId) -> Result<(), StorageError>;
    fn delete_scanned_epochs_older_than(&mut self, epoch: Epoch) -> Result<(), StorageError>;
    fn insert_or_ignore_transaction(&mut self, transaction: &Transaction) -> Result<(), StorageError>;
    fn insert_or_ignore_epoch_checkpoint(&mut self, epoch_checkpoint: &EpochCheckpoint) -> Result<(), StorageError>;
}

impl IndexerStoreWriteTransaction for SqliteStoreWriteTransaction<'_> {
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
        use schema::key_values;
        let json = serialize_json(&value)?;

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
        use schema::substate_transitions;

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
                                .map(|v| serialize_hex(v.substate.value.to_value_hash(proof.version())))),
                            // substate_transitions::utxo_tag_byte.eq(proof
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .execute(self.connection())
            .map_err(|e| StorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn batch_insert_utxo_updates<I: IntoIterator<Item = UtxoUpdate>>(
        &mut self,
        updates: I,
    ) -> Result<(), StorageError> {
        const OPERATION: &str = "batch_insert_utxo_updates";
        use crate::storage_sqlite::schema::utxos;

        fn utxo_exists(connection: &mut SqliteConnection, address: &str) -> Result<bool, StorageError> {
            use crate::storage_sqlite::schema::utxos;

            let count = utxos::table
                .filter(utxos::address.eq(address))
                .count()
                .limit(1)
                .get_result::<i64>(connection)
                .map_err(|e| StorageError::QueryError {
                    reason: format!("utxo_exists: {}", e),
                })?;
            Ok(count > 0)
        }

        let (inserts, updates) =
            updates
                .into_iter()
                .try_fold((Vec::new(), Vec::new()), |(mut inserts, mut updates), update| {
                    match update {
                        UtxoUpdate::Unspent(unspent) => {
                            let address = unspent.address.to_string();
                            if utxo_exists(self.connection(), &address)? {
                                // If the UTXO already exists, we update it instead of inserting a new one
                                let update = UtxoRecordUpdate {
                                    version: Some(unspent.version as i32),
                                    output: Some(unspent.utxo.output().map(serialize_json).transpose()?),
                                    state_version: Some(unspent.state_version.as_u64() as i64),
                                    is_spent: Some(false),
                                    is_burnt: Some(unspent.utxo.is_burnt()),
                                    is_frozen: Some(unspent.utxo.is_frozen()),
                                };
                                updates.push((address, update));
                            } else {
                                inserts.push(UtxoRecordInsert {
                                    address,
                                    version: unspent.version as i32,
                                    output: unspent.utxo.output().map(serialize_json).transpose()?,
                                    shard: unspent.shard.as_u32() as i32,
                                    resource_address: unspent.address.resource_address().to_string(),
                                    state_version: unspent.state_version.as_u64() as i64,
                                    utxo_tag_byte: unspent.utxo.output().map(|o| i32::from(o.tag.as_byte())),
                                    is_spent: false,
                                    is_burnt: unspent.utxo.is_burnt(),
                                    is_frozen: unspent.utxo.is_frozen(),
                                });
                            }
                        },
                        UtxoUpdate::Spent(spent) => {
                            let update = UtxoRecordUpdate {
                                version: Some(spent.version as i32),
                                // Prune the UTXO data for spent outputs
                                output: Some(None),
                                // Update to deleted state version
                                state_version: Some(spent.state_version.as_u64() as i64),
                                is_spent: Some(true),
                                ..Default::default()
                            };
                            updates.push((spent.address.to_string(), update));
                        },
                    }

                    Ok::<_, StorageError>((inserts, updates))
                })?;

        for insert in inserts {
            // batch insert results in "cannot start a transaction within a transaction" error
            diesel::insert_into(utxos::table)
                .values(insert)
                .execute(self.connection())
                .map_err(|e| StorageError::general(OPERATION, format!("insert error: {e}")))?;
        }

        for (address, update) in updates {
            diesel::update(utxos::table.filter(utxos::address.eq(address)))
                .set(update)
                .execute(self.connection())
                .map_err(|e| StorageError::general(OPERATION, format!("update error: {e}")))?;
        }

        Ok(())
    }

    fn upsert_substate(&mut self, new_substate: NewSubstate) -> Result<(), StorageError> {
        use crate::storage_sqlite::schema::substates;

        let address = &new_substate.address;
        let current_substate = substates::table
            .filter(substates::address.eq(address))
            .first::<Substate>(self.connection())
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

    fn batch_insert_events<I: IntoIterator<Item = Event>>(&mut self, events: I) -> Result<(), StorageError> {
        const OPERATION: &str = "batch_insert_events";
        use crate::storage_sqlite::schema::events;

        let events = events.into_iter().map(|event| {
            Ok::<_, StorageError>(NewEvent {
                template_address: event.template_address().to_string(),
                tx_hash: event.tx_hash().to_string(),
                topic: event.topic(),
                payload: serialize_json(event.payload())?,
                substate_id: event.substate_id().map(|s| s.to_string()),
            })
        });

        for result in events {
            let event = result?;

            diesel::insert_into(events::table)
                .values(event)
                .execute(self.connection())
                .map_err(|e| StorageError::QueryError {
                    reason: format!("{OPERATION}: {}", e),
                })?;
        }

        Ok(())
    }

    fn save_scanned_block_id(&mut self, new: NewScannedBlockId) -> Result<(), StorageError> {
        use crate::storage_sqlite::schema::scanned_block_ids;

        diesel::insert_into(scanned_block_ids::table)
            .values(&new)
            .on_conflict((scanned_block_ids::epoch, scanned_block_ids::shard_group))
            .do_update()
            .set(new.clone())
            .execute(&mut *self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("save_scanned_block_id error: {}", e),
            })?;

        debug!(
            target: LOG_TARGET,
            "Added new scanned block id {} for epoch {} and shard {:?}", to_hex(&new.last_block_id), new.epoch, new.shard_group
        );

        Ok(())
    }

    fn delete_scanned_epochs_older_than(&mut self, epoch: Epoch) -> Result<(), StorageError> {
        use crate::storage_sqlite::schema::scanned_block_ids;

        diesel::delete(scanned_block_ids::table)
            .filter(scanned_block_ids::epoch.lt(epoch.as_u64() as i64))
            .execute(&mut *self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("delete_scanned_epochs_older_than: {}", e),
            })?;

        Ok(())
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
}

impl<'a> Deref for SqliteStoreWriteTransaction<'a> {
    type Target = SqliteStoreReadTransaction<'a>;

    fn deref(&self) -> &Self::Target {
        self.transaction.as_ref().unwrap()
    }
}

impl DerefMut for SqliteStoreWriteTransaction<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.transaction.as_mut().unwrap()
    }
}

impl Drop for SqliteStoreWriteTransaction<'_> {
    fn drop(&mut self) {
        if self.transaction.is_some() {
            warn!(
                target: LOG_TARGET,
                "Substate store write transaction was not committed/rolled back"
            );
        }
    }
}
