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
    collections::BTreeMap,
    fs::create_dir_all,
    ops::{Deref, DerefMut},
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};

use diesel::{prelude::*, sql_query, SqliteConnection};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness};
use log::*;
use tari_consensus_types::BlockId;
use tari_crypto::tari_utilities::hex::to_hex;
use tari_engine_types::substate::{SubstateId, SubstateValue};
use tari_indexer_client::types::ListSubstateItem;
use tari_indexer_lib::NonFungibleSubstate;
use tari_ootle_common_types::{substate_type::SubstateType, Epoch, ShardGroup};
use tari_ootle_storage::StorageError;
use tari_ootle_storage_sqlite::{error::SqliteStorageError, SqliteTransaction};
use tari_template_lib::{models::ResourceAddress, types::TemplateAddress};
use thiserror::Error;

use super::models::events::{NewEvent, NewScannedBlockId};
use crate::substate_storage_sqlite::models::{
    events::{Event, NewEventPayloadField, ScannedBlockId},
    substate::{NewSubstate, Substate},
};

const LOG_TARGET: &str = "tari::indexer::substate_storage_sqlite";

#[derive(Clone)]
pub struct SqliteSubstateStore {
    connection: Arc<Mutex<SqliteConnection>>,
}

impl SqliteSubstateStore {
    pub fn try_create(path: PathBuf) -> Result<Self, StorageError> {
        create_dir_all(path.parent().unwrap()).map_err(|_| StorageError::FileSystemPathDoesNotExist)?;

        let database_url = path.to_str().expect("database_url utf-8 error").to_string();
        let mut connection = SqliteConnection::establish(&database_url).map_err(SqliteStorageError::from)?;

        pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./src/substate_storage_sqlite/migrations");
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

    pub fn find_by_address(address: String, conn: &mut SqliteConnection) -> Result<Option<Substate>, StorageError> {
        use crate::substate_storage_sqlite::schema::substates;

        let substate_option = substates::table
            .filter(substates::address.eq(address))
            .first(&mut *conn)
            .optional()
            .map_err(|e| StorageError::QueryError {
                reason: format!("find_by_address: {}", e),
            })?;

        Ok(substate_option)
    }
}
pub trait SubstateStore {
    type ReadTransaction<'a>: SubstateStoreReadTransaction
    where Self: 'a;
    type WriteTransaction<'a>: SubstateStoreWriteTransaction + Deref<Target = Self::ReadTransaction<'a>>
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

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("Storage error: {details}")]
    StorageError { details: String },
}

impl From<StorageError> for StoreError {
    fn from(err: StorageError) -> Self {
        Self::StorageError {
            details: err.to_string(),
        }
    }
}

impl SubstateStore for SqliteSubstateStore {
    type ReadTransaction<'a> = SqliteSubstateStoreReadTransaction<'a>;
    type WriteTransaction<'a> = SqliteSubstateStoreWriteTransaction<'a>;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, StorageError> {
        let tx = SqliteTransaction::begin(self.connection.lock().unwrap())?;
        Ok(SqliteSubstateStoreReadTransaction::new(tx))
    }

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, StorageError> {
        let tx = SqliteTransaction::begin(self.connection.lock().unwrap())?;
        Ok(SqliteSubstateStoreWriteTransaction::new(tx))
    }
}

pub struct SqliteSubstateStoreReadTransaction<'a> {
    transaction: SqliteTransaction<'a>,
}

impl<'a> SqliteSubstateStoreReadTransaction<'a> {
    fn new(transaction: SqliteTransaction<'a>) -> Self {
        Self { transaction }
    }

    fn connection(&mut self) -> &mut SqliteConnection {
        self.transaction.connection()
    }
}

pub trait SubstateStoreReadTransaction {
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
    ) -> Result<Vec<Event>, StorageError>;
    fn event_exists(&mut self, event: &NewEvent) -> Result<bool, StorageError>;
    fn get_oldest_scanned_epoch(&mut self) -> Result<Option<Epoch>, StorageError>;
    fn get_last_scanned_block_id(
        &mut self,
        epoch: Epoch,
        shard_group: ShardGroup,
    ) -> Result<Option<BlockId>, StorageError>;
}

impl SubstateStoreReadTransaction for SqliteSubstateStoreReadTransaction<'_> {
    fn list_substates(
        &mut self,
        by_type: Option<SubstateType>,
        by_template_address: Option<TemplateAddress>,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<ListSubstateItem>, StorageError> {
        use crate::substate_storage_sqlite::schema::substates;

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
                let version = u32::try_from(s.version)?;
                let template_address = s.template_address.map(|h| TemplateAddress::from_hex(&h)).transpose()?;
                let timestamp = u64::try_from(s.timestamp)?;
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
        use crate::substate_storage_sqlite::schema::substates;

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
        use crate::substate_storage_sqlite::schema::non_fungible_indexes;

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
        use crate::substate_storage_sqlite::schema::substates;

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

                let substate = tari_engine_types::substate::Substate::new(row.version as u32, value);

                Ok(NonFungibleSubstate {
                    address: row.address.parse().map_err(|e| StorageError::DataInconsistency {
                        details: format!("Failed to parse address: {}", e),
                    })?,
                    // TODO: improve the indexer api. The index is not used.
                    index: row.version.try_into().map_err(|e| StorageError::DataInconsistency {
                        details: format!("Version overflow {}", e),
                    })?,
                    substate,
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
    ) -> Result<Vec<Event>, StorageError> {
        // TODO: allow to query by payload as well, unifying all event methods into one
        info!(
            target: LOG_TARGET,
            "Querying substate scanner database: get_events with substate_id_filter = {:?} and \
            topic_filter = {:?}",
            substate_id_filter,
            topic_filter
        );
        use crate::substate_storage_sqlite::schema::events;

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
            .get_results::<Event>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("get_events: {}", e),
            })?;

        Ok(events)
    }

    fn event_exists(&mut self, value: &NewEvent) -> Result<bool, StorageError> {
        use crate::substate_storage_sqlite::schema::events;

        let count = events::table
            .filter(
                events::substate_id
                    .eq(&value.substate_id)
                    .and(events::template_address.eq(&value.template_address))
                    .and(events::topic.eq(&value.topic))
                    .and(events::version.eq(&value.version))
                    .and(events::payload.eq(&value.payload))
                    .and(events::tx_hash.eq(&value.tx_hash)),
            )
            .count()
            .get_result::<i64>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("event_exists: {}", e),
            })?;
        let exists = count > 0;

        Ok(exists)
    }

    fn get_oldest_scanned_epoch(&mut self) -> Result<Option<Epoch>, StorageError> {
        use crate::substate_storage_sqlite::schema::scanned_block_ids;

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
        use crate::substate_storage_sqlite::schema::scanned_block_ids;

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
}

pub struct SqliteSubstateStoreWriteTransaction<'a> {
    /// None indicates if the transaction has been explicitly committed/rolled back
    transaction: Option<SqliteSubstateStoreReadTransaction<'a>>,
}

impl<'a> SqliteSubstateStoreWriteTransaction<'a> {
    pub fn new(transaction: SqliteTransaction<'a>) -> Self {
        Self {
            transaction: Some(SqliteSubstateStoreReadTransaction::new(transaction)),
        }
    }

    pub fn connection(&mut self) -> &mut SqliteConnection {
        self.transaction.as_mut().unwrap().connection()
    }
}

// TODO: remove the allow dead_code attributes as these become used.
pub trait SubstateStoreWriteTransaction {
    fn commit(self) -> Result<(), StorageError>;
    fn rollback(self) -> Result<(), StorageError>;
    fn set_substate(&mut self, new_substate: NewSubstate) -> Result<(), StorageError>;
    #[allow(dead_code)]
    fn delete_substate(&mut self, address: String) -> Result<(), StorageError>;
    #[allow(dead_code)]
    fn clear_substates(&mut self) -> Result<(), StorageError>;
    fn save_event(&mut self, new_event: NewEvent) -> Result<(), StorageError>;
    fn save_scanned_block_id(&mut self, new_scanned_block_id: NewScannedBlockId) -> Result<(), StorageError>;
    fn delete_scanned_epochs_older_than(&mut self, epoch: Epoch) -> Result<(), StorageError>;
}

impl SubstateStoreWriteTransaction for SqliteSubstateStoreWriteTransaction<'_> {
    fn commit(mut self) -> Result<(), StorageError> {
        self.transaction.take().unwrap().transaction.commit()?;
        Ok(())
    }

    fn rollback(mut self) -> Result<(), StorageError> {
        self.transaction.take().unwrap().transaction.rollback()?;
        Ok(())
    }

    fn set_substate(&mut self, new_substate: NewSubstate) -> Result<(), StorageError> {
        use crate::substate_storage_sqlite::schema::substates;

        let address = &new_substate.address;
        let conn = self.connection();
        let current_substate = SqliteSubstateStore::find_by_address(address.clone(), conn)?;

        match current_substate {
            Some(_) => {
                diesel::update(substates::table)
                    .set(&new_substate)
                    .filter(substates::address.eq(address))
                    .execute(&mut *conn)
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
                    .execute(&mut *conn)
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

    fn delete_substate(&mut self, address: String) -> Result<(), StorageError> {
        use crate::substate_storage_sqlite::schema::substates;

        diesel::delete(substates::table)
            .filter(substates::address.eq(address))
            .execute(&mut *self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("delete substate error: {}", e),
            })?;

        Ok(())
    }

    fn clear_substates(&mut self) -> Result<(), StorageError> {
        use crate::substate_storage_sqlite::schema::substates;

        diesel::delete(substates::table)
            .execute(&mut *self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("clear_substates error: {}", e),
            })?;

        Ok(())
    }

    fn save_event(&mut self, new_event: NewEvent) -> Result<(), StorageError> {
        use crate::substate_storage_sqlite::schema::{event_payloads, events};

        // Save the event into the database
        let event_row: Event = diesel::insert_into(events::table)
            .values(&new_event)
            .get_result::<Event>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("save_event: {}", e),
            })?;

        // Save all the key-value pairs of the payload to be able to query them later
        let payload: BTreeMap<String, String> =
            serde_json::from_str(new_event.payload.as_str()).map_err(|e| StorageError::QueryError {
                reason: format!("save_event: {}", e),
            })?;
        let new_payload_fields = payload
            .into_iter()
            .map(|(key, value)| NewEventPayloadField {
                payload_key: key,
                payload_value: value,
                event_id: event_row.id,
            })
            .collect::<Vec<_>>();
        // diesel fails if we try to pass all the new rows in a single insert
        // so the workaround is to loop over them
        // TODO: make diesel work with a single insert instruction instead of looping
        for field in new_payload_fields {
            diesel::insert_into(event_payloads::table)
                .values(&field)
                .execute(self.connection())
                .map_err(|e| StorageError::QueryError {
                    reason: format!("save_event: {}", e),
                })?;
        }
        debug!(
            target: LOG_TARGET,
            "Added new event to the database with substate_id = {:?}, template_address = {} and for transaction \
             hash = {}, version = {}",
            new_event.substate_id,
            new_event.template_address,
            new_event.tx_hash,
            new_event.version,
        );

        Ok(())
    }

    fn save_scanned_block_id(&mut self, new: NewScannedBlockId) -> Result<(), StorageError> {
        use crate::substate_storage_sqlite::schema::scanned_block_ids;

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
        use crate::substate_storage_sqlite::schema::scanned_block_ids;

        diesel::delete(scanned_block_ids::table)
            .filter(scanned_block_ids::epoch.lt(epoch.as_u64() as i64))
            .execute(&mut *self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("delete_scanned_epochs_older_than: {}", e),
            })?;

        Ok(())
    }
}

impl<'a> Deref for SqliteSubstateStoreWriteTransaction<'a> {
    type Target = SqliteSubstateStoreReadTransaction<'a>;

    fn deref(&self) -> &Self::Target {
        self.transaction.as_ref().unwrap()
    }
}

impl DerefMut for SqliteSubstateStoreWriteTransaction<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.transaction.as_mut().unwrap()
    }
}

impl Drop for SqliteSubstateStoreWriteTransaction<'_> {
    fn drop(&mut self) {
        if self.transaction.is_some() {
            warn!(
                target: LOG_TARGET,
                "Substate store write transaction was not committed/rolled back"
            );
        }
    }
}
