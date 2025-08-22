//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use diesel::internal::derives::multiconnection::time::PrimitiveDateTime;
use tari_engine_types::{substate::SubstateId, Utxo};
use tari_indexer_client::types::{UtxoSpent, UtxoUnspent, UtxoUpdate};
use tari_ootle_common_types::{shard::Shard, StateVersion, VersionedSubstateId};
use tari_ootle_storage::StorageError;

use crate::storage_sqlite::{schema::utxos, serialization::deserialize_json};

#[derive(AsChangeset, Default)]
#[diesel(table_name = utxos)]
pub(crate) struct UtxoRecordUpdate {
    pub version: Option<i32>,
    pub output: Option<Option<String>>,
    pub state_version: Option<i64>,
    pub is_spent: Option<bool>,
    pub is_burnt: Option<bool>,
    pub is_frozen: Option<bool>,
}

#[derive(Insertable)]
#[diesel(table_name = utxos)]
pub(crate) struct UtxoRecordInsert {
    pub substate_id: String,
    pub version: i32,
    pub shard: i32,
    pub resource_address: String,
    pub state_version: i64,
    pub output: Option<String>,
    pub utxo_tag_byte: Option<i32>,
    pub is_spent: bool,
    pub is_burnt: bool,
    pub is_frozen: bool,
}
#[derive(Queryable)]
#[diesel(table_name = utxos)]
pub(crate) struct UtxoRecord {
    #[allow(dead_code)]
    pub id: i32,
    pub substate_id: String,
    pub version: i32,
    #[allow(dead_code)]
    pub resource_address: String,
    pub shard: i32,
    pub state_version: i64,
    pub output: Option<String>,
    #[allow(dead_code)]
    pub utxo_tag_byte: Option<i32>,
    pub is_spent: bool,
    #[allow(dead_code)]
    pub is_burnt: bool,
    pub is_frozen: bool,
    #[allow(dead_code)]
    pub created_at: PrimitiveDateTime,
}

impl UtxoRecord {
    pub fn try_convert(self) -> Result<UtxoUpdate, StorageError> {
        let substate_id = SubstateId::from_str(&self.substate_id).map_err(|e| StorageError::DecodingError {
            operation: "UtxoRecord::try_convert",
            item: "Utxo",
            details: format!("Failed to parse SubstateId from string: {}", e),
        })?;
        let substate_id = VersionedSubstateId::new(substate_id, self.version as u32);

        let update = if self.is_spent {
            UtxoUpdate::Spent(UtxoSpent {
                versioned_substate_id: substate_id,
                state_version: StateVersion::new(self.state_version as u64),
            })
        } else {
            UtxoUpdate::Unspent(UtxoUnspent {
                versioned_substate_id: substate_id,
                shard: Shard::from(self.shard as u32),
                state_version: StateVersion::new(self.state_version as u64),
                utxo: Utxo {
                    output: self.output.as_ref().map(deserialize_json).transpose().map_err(|e| {
                        StorageError::DecodingError {
                            operation: "UtxoRecord::try_convert",
                            item: "Utxo",
                            details: format!("Failed to parse Utxo from string: {}", e),
                        }
                    })?,
                    is_frozen: self.is_frozen,
                },
            })
        };

        Ok(update)
    }
}
