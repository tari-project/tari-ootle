//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Write, str::FromStr};

use diesel::{
    dsl,
    sql_types,
    BoolExpressionMethods,
    ExpressionMethods,
    NullableExpressionMethods,
    OptionalExtension,
    QueryDsl,
    RunQueryDsl,
    SqliteConnection,
    TextExpressionMethods,
};
use log::info;
use serde::de::DeserializeOwned;
use tari_consensus_types::BlockId;
use tari_engine_types::{
    events::Event,
    substate::{SubstateId, SubstateValue},
    transaction_receipt::{TransactionReceipt, TransactionReceiptAddress},
    Utxo,
};
use tari_indexer_client::types::{ListSubstateItem, NonFungibleSubstate, TransactionEntry};
use tari_ootle_common_types::{
    displayable::Displayable,
    shard::Shard,
    substate_type::SubstateType,
    Epoch,
    ShardGroup,
    StateVersion,
};
use tari_ootle_storage::{time::PrimitiveDateTime, Ordering, StorageError};
use tari_ootle_storage_sqlite::SqliteTransaction;
use tari_ootle_wallet_sdk::models::WalletUtxoUpdate;
use tari_template_lib::{
    models::{ResourceAddress, UtxoId},
    prelude::{RistrettoPublicKeyBytes, TemplateAddress},
    types::{crypto::UtxoTag, Hash},
};
use tari_transaction::{Transaction, TransactionId};

use crate::{
    storage_sqlite::{
        models,
        models::{EventRecord, KeyValue, ScannedBlockId, SubstateRecord},
        serialization::{deserialize_hex_try_from, deserialize_json, serialize_hex},
    },
    store::IndexerStoreReadTransaction,
    substate_manager::SubstateResponse,
};

const LOG_TARGET: &str = "tari::indexer::storage_sqlite::reader";

pub struct SqliteStoreReadTransaction<'a> {
    pub(super) transaction: SqliteTransaction<'a>,
}

impl<'a> SqliteStoreReadTransaction<'a> {
    pub(super) fn new(transaction: SqliteTransaction<'a>) -> Self {
        Self { transaction }
    }

    pub(super) fn connection(&mut self) -> &mut SqliteConnection {
        self.transaction.connection()
    }
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

        let substates = query
            .order_by(substates::id.desc())
            .load_iter::<SubstateRecord, _>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("list_substates: {}", e),
            })?;

        let items = substates
            .into_iter()
            .map(|res| {
                let s = res.map_err(|e| StorageError::QueryError {
                    reason: format!("list_substates: {}", e),
                })?;
                let substate_id = SubstateId::from_str(&s.address).map_err(|e| StorageError::DataInconsistency {
                    details: format!("Invalid substate address {}: {}", s.address, e),
                })?;
                let version = s.version as u32;
                let template_address = s.template_address.map(|h| deserialize_hex_try_from(&h)).transpose()?;
                let timestamp = s.updated_at;
                Ok(ListSubstateItem {
                    substate_id,
                    module_name: s.module_name,
                    version,
                    template_address,
                    timestamp,
                })
            })
            .collect::<Result<Vec<ListSubstateItem>, StorageError>>()
            .map_err(|e| StorageError::QueryError {
                reason: format!("list_substates: invalid substate items: {}", e),
            })?;

        Ok(items)
    }

    fn get_substate(
        &mut self,
        address: &SubstateId,
        version: Option<u32>,
    ) -> Result<Option<SubstateRecord>, StorageError> {
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

    fn get_substates(&mut self, ids: &[SubstateId]) -> Result<Vec<SubstateResponse>, StorageError> {
        use crate::storage_sqlite::schema::substates;

        let str_ids = ids.iter().map(|id| id.to_string());

        let rows = substates::table
            .select(substates::all_columns)
            .filter(substates::address.eq_any(str_ids))
            .load_iter::<SubstateRecord, _>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("get_substates: {}", e),
            })?;

        rows.into_iter()
            .map(|res| {
                res.map_err(|e| StorageError::QueryError {
                    reason: format!("get_substates: {e}"),
                })
                .and_then(TryInto::try_into)
            })
            .collect()
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
            .load_iter::<SubstateRecord, _>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("get_non_fungibles_by_resource_address: {}", e),
            })?;

        res.into_iter()
            .map(|res| {
                let row = res.map_err(|e| StorageError::QueryError {
                    reason: format!("get_non_fungibles_by_resource_address: {}", e),
                })?;
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
        substate_id_filter: Option<&SubstateId>,
        topic_filter: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<(TransactionId, Event)>, StorageError> {
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

        let event_rows = query
            .offset(offset.into())
            .limit(limit.into())
            .order(events::id.desc())
            .load_iter::<EventRecord, _>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("get_events: {}", e),
            })?;

        event_rows
            .map(|res| {
                res.map_err(|e| StorageError::QueryError {
                    reason: format!("get_events: {}", e),
                })
                .and_then(|row| {
                    let substate_id = row
                        .substate_id
                        .as_ref()
                        .map(|str| SubstateId::from_str(str))
                        .transpose()
                        .map_err(|e| StorageError::DataInconsistency {
                            details: format!(
                                "Invalid substate_id {} in events table: {}",
                                row.substate_id.display(),
                                e
                            ),
                        })?;
                    let template_address =
                        Hash::from_hex(&row.template_address).map_err(|e| StorageError::DataInconsistency {
                            details: format!(
                                "Invalid template_address {} in events table: {}",
                                row.template_address, e
                            ),
                        })?;
                    let tx_hash = Hash::from_hex(&row.tx_hash).map_err(|e| StorageError::DataInconsistency {
                        details: format!("Invalid tx_hash {} in events table: {}", row.tx_hash, e),
                    })?;
                    let topic = row.topic;
                    let payload = deserialize_json(&row.payload)?;
                    Ok((
                        TransactionId::from(tx_hash),
                        Event::new(substate_id, template_address, topic, payload),
                    ))
                })
            })
            .collect()
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

    fn list_transaction_receipts(
        &mut self,
        last_id: Option<TransactionReceiptAddress>,
        limit: u64,
        ordering: Ordering,
    ) -> Result<Vec<(TransactionReceiptAddress, TransactionReceipt)>, StorageError> {
        const OPERATION: &str = "list_transaction_receipts";
        use crate::storage_sqlite::schema::transaction_receipts;

        let mut query = transaction_receipts::table
            .select((transaction_receipts::address, transaction_receipts::data))
            .into_boxed();
        if let Some(last_id) = last_id {
            let tr = alias!(transaction_receipts as tr);
            let subquery = tr
                .select(tr.fields(transaction_receipts::id))
                .filter(transaction_receipts::address.eq(last_id.to_string()))
                .limit(1)
                .single_value()
                .assume_not_null();

            query = match ordering {
                Ordering::Ascending => query.filter(transaction_receipts::id.gt(subquery)),
                Ordering::Descending => query.filter(transaction_receipts::id.lt(subquery)),
            }
        }

        query = match ordering {
            Ordering::Ascending => query.order_by(transaction_receipts::address.asc()),
            Ordering::Descending => query.order_by(transaction_receipts::address.desc()),
        };

        let rows = query
            .limit(limit as i64)
            .load_iter::<(String, String), _>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("{OPERATION}: {}", e),
            })?;

        rows.into_iter()
            .map(|row| {
                row.map_err(|e| StorageError::QueryError {
                    reason: format!("{OPERATION}: {}", e),
                })
                .and_then(|(addr, data)| {
                    Ok((
                        TransactionReceiptAddress::from_str(&addr).map_err(|e| StorageError::DataInconsistency {
                            details: format!("Invalid transaction receipt address {}: {}", addr, e),
                        })?,
                        deserialize_json(data)?,
                    ))
                })
            })
            .collect()
    }

    fn get_transaction_receipt(
        &mut self,
        address: &TransactionReceiptAddress,
    ) -> Result<TransactionReceipt, StorageError> {
        const OPERATION: &str = "get_transaction_receipt";
        use crate::storage_sqlite::schema::transaction_receipts;

        let receipt_entry = transaction_receipts::table
            .select(transaction_receipts::data)
            .filter(transaction_receipts::address.eq(address.to_string()))
            .first::<String>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("{OPERATION}: {}", e),
            })?;

        deserialize_json(&receipt_entry)
    }

    // -------------------------------- KeyValues -------------------------------- //
    fn key_value_get_value<K: AsRef<str>, T: DeserializeOwned>(&mut self, key: K) -> Result<T, StorageError> {
        let key_value = self.key_value_get_raw(key)?;
        deserialize_json(&key_value.value)
    }

    fn key_value_get_raw<K: AsRef<str>>(&mut self, key: K) -> Result<KeyValue<String>, StorageError> {
        const OPERATION: &str = "key_value_get_json";
        use crate::storage_sqlite::schema::key_values;

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

    fn utxos_get_max_state_version(
        &mut self,
        resource_address: ResourceAddress,
        shard: Shard,
    ) -> Result<StateVersion, StorageError> {
        const OPERATION: &str = "utxos_get_max_state_version";
        use crate::storage_sqlite::schema::utxos;
        let max_version = utxos::table
            .select(dsl::max(utxos::state_version))
            .filter(utxos::resource_address.eq(resource_address.to_string()))
            .filter(utxos::shard.eq(shard.as_u32() as i32))
            .get_result::<Option<i64>>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("{OPERATION}: {}", e),
            })?
            .map(|v| StateVersion::new(v as u64))
            .unwrap_or_else(StateVersion::zero);

        Ok(max_version)
    }

    fn utxos_get_updates(
        &mut self,
        resource_address: ResourceAddress,
        shard: Shard,
        from_state_version: StateVersion,
        unspent_only: bool,
        limit: u32,
    ) -> Result<(StateVersion, Vec<WalletUtxoUpdate>), StorageError> {
        const OPERATION: &str = "get_utxo_updates";
        use crate::storage_sqlite::schema::utxos;

        let mut query = utxos::table
            .filter(utxos::resource_address.eq(resource_address.to_string()))
            .filter(utxos::state_version.gt(from_state_version.as_u64() as i64))
            .filter(utxos::shard.eq(shard.as_u32() as i32))
            .limit(i64::from(limit))
            .order_by(utxos::state_version.asc())
            .into_boxed();
        if unspent_only {
            // Only return unspent UTXOs
            query = query
                .filter(utxos::is_spent.eq(false))
                .filter(utxos::is_burnt.eq(false));
        }
        let rows = query
            .load_iter::<models::UtxoRecord, _>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("{OPERATION}: {}", e),
            })?;

        let mut updates = Vec::new();
        let mut max_state_version = StateVersion::zero();
        for row in rows {
            let row = row.map_err(|e| StorageError::QueryError {
                reason: format!("{OPERATION}: {}", e),
            })?;
            let (state_version, update) = row.try_convert_to_update()?;
            max_state_version = max_state_version.max(state_version);
            updates.push(update);
        }

        Ok((max_state_version, updates))
    }

    fn utxos_get_unspent_by_public_nonce_and_tag(
        &mut self,
        resource_address: &ResourceAddress,
        tag_and_nonce_pairs: &[(UtxoTag, RistrettoPublicKeyBytes)],
    ) -> Result<Vec<(UtxoId, Utxo)>, StorageError> {
        const OPERATION: &str = "utxos_get_unspent_by_public_nonce_and_tag";
        use crate::storage_sqlite::schema::utxos;

        // Diesel does not support tuple IN queries, so we have to use raw SQL here (https://github.com/diesel-rs/diesel/issues/3222#issuecomment-2088239186)
        let mut tag_and_nonce_query = String::with_capacity(29 + tag_and_nonce_pairs.len() * 50);
        tag_and_nonce_query.push_str("(utxo_tag,public_nonce) IN (");
        for (i, (tag, nonce)) in tag_and_nonce_pairs.iter().enumerate() {
            // SQLINJECTION not possible as we control the types
            write!(
                &mut tag_and_nonce_query,
                "({},'{}')",
                tag.value() as i32,
                serialize_hex(nonce)
            )
            .expect("write to string is infallible");

            if i < tag_and_nonce_pairs.len() - 1 {
                tag_and_nonce_query.push(',');
            }
        }
        tag_and_nonce_query.push(')');

        let rows = utxos::table
            .filter(utxos::resource_address.eq(resource_address.to_string()))
            .filter(dsl::sql::<sql_types::Bool>(&tag_and_nonce_query))
            .filter(utxos::is_spent.eq(false))
            .filter(utxos::is_burnt.eq(false))
            .load_iter::<models::UtxoRecord, _>(self.connection())
            .map_err(|e| StorageError::QueryError {
                reason: format!("{OPERATION}: {}", e),
            })?;
        let mut utxos = Vec::new();
        for row in rows {
            let row = row.map_err(|e| StorageError::QueryError {
                reason: format!("{OPERATION}: {}", e),
            })?;

            let (address, utxo) = row.try_convert_to_utxo()?;
            utxos.push((*address.id(), utxo));
        }

        Ok(utxos)
    }
}
