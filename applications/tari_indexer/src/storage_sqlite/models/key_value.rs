//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_storage::time::PrimitiveDateTime;

use crate::storage_sqlite::schema::key_values;

pub struct KeyValue<T> {
    pub _key: String,
    pub value: T,
    pub _created_at: PrimitiveDateTime,
    pub _updated_at: PrimitiveDateTime,
}

impl From<KeyValueEntry> for KeyValue<String> {
    fn from(entry: KeyValueEntry) -> Self {
        KeyValue {
            _key: entry.key,
            value: entry.value,
            _created_at: entry.created_at,
            _updated_at: entry.updated_at,
        }
    }
}

#[derive(Debug, Identifiable, Queryable)]
#[diesel(table_name = key_values)]
pub(crate) struct KeyValueEntry {
    pub id: i32,
    pub key: String,
    pub value: String,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

pub enum Key {
    /// The network that applies to this database. Used to determine if the network has changed.
    /// type: Network
    Network,
    /// A summary of the sync progress. Used to resume sync after restarts.
    /// type: SyncProgress
    SyncProgress,
    /// The total accumulated amount of XTR that has been burned as exhaust.
    /// type: Amount
    XtrAccumulatedExhaustBurn,
    /// The total accumulated amount of XTR that has been claimed.
    /// type: Amount
    XtrAccumulatedClaimed,
}

impl Key {
    pub fn as_key_str(&self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::SyncProgress => "sync_progress",
            Self::XtrAccumulatedClaimed => "xtr_accumulated_claimed",
            Self::XtrAccumulatedExhaustBurn => "xtr_accumulated_exhaust_burn",
        }
    }
}

impl AsRef<str> for Key {
    fn as_ref(&self) -> &str {
        self.as_key_str()
    }
}
