// Copyright 2022 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause
#[macro_use]
extern crate diesel;

mod models;
mod reader;
mod schema;
mod serialization;
mod writer;

use std::{
    fmt::{Debug, Formatter},
    fs::create_dir_all,
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use diesel::{Connection, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl, SqliteConnection, sql_query};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use tari_ootle_wallet_sdk::storage::{ReadableWalletStore, WalletStorageError, WriteableWalletStore};

use crate::{models::api_key::{ApiKey, NewApiKey}, reader::ReadTransaction, writer::WriteTransaction};

#[derive(Clone)]
pub struct SqliteWalletStore {
    // MUTEX: required to make Sync
    connection: Arc<Mutex<SqliteConnection>>,
}

impl SqliteWalletStore {
    pub fn try_open<P: AsRef<Path>>(path: P) -> Result<Self, WalletStorageError> {
        create_dir_all(path.as_ref().parent().unwrap()).expect("Failed to create DB path");

        let database_url = path.as_ref().to_str().expect("database_url utf-8 error").to_string();
        let mut connection =
            SqliteConnection::establish(&database_url).map_err(|e| WalletStorageError::general("connect", e))?;

        sql_query("PRAGMA foreign_keys = ON;")
            .execute(&mut connection)
            .map_err(|source| WalletStorageError::general("set pragma", source))?;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn run_migrations(&self) -> Result<(), WalletStorageError> {
        let mut conn = self.connection.lock().unwrap();
        const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations");
        conn.run_pending_migrations(MIGRATIONS)
            .map_err(|source| WalletStorageError::general("migrate", source))?;

        Ok(())
    }
}

impl ReadableWalletStore for SqliteWalletStore {
    type ReadTransaction<'a> = ReadTransaction<'a>;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, WalletStorageError> {
        let mut lock = self.connection.lock().unwrap();
        sql_query("BEGIN")
            .execute(&mut *lock)
            .map_err(|e| WalletStorageError::general("BEGIN transaction", e))?;
        Ok(ReadTransaction::new(lock))
    }
}

impl WriteableWalletStore for SqliteWalletStore {
    type WriteTransaction<'a> = WriteTransaction<'a>;

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, WalletStorageError> {
        let mut lock = self.connection.lock().unwrap();
        sql_query("BEGIN")
            .execute(&mut *lock)
            .map_err(|e| WalletStorageError::general("BEGIN transaction", e))?;
        Ok(WriteTransaction::new(lock))
    }
}

impl Debug for SqliteWalletStore {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteWalletStore")
            .field("connection", &"SqliteConnection")
            .finish()
    }
}

// ─── API-key storage extension ────────────────────────────────────────────────
// Keeps api_key CRUD out of the SDK trait layer so no core trait changes needed.

/// Re-export the raw DB row type for callers that need it (e.g. walletd handlers).
pub use models::api_key::ApiKey as ApiKeyRow;

pub trait ApiKeyStore {
    fn api_keys_insert(&mut self, name: &str, key_hash: &[u8], permissions: &str) -> Result<ApiKey, WalletStorageError>;
    fn api_keys_get_by_hash(&mut self, key_hash: &[u8]) -> Result<ApiKey, WalletStorageError>;
    fn api_keys_list_all(&mut self) -> Result<Vec<ApiKey>, WalletStorageError>;
    fn api_keys_revoke(&mut self, id: i32, now: i64) -> Result<(), WalletStorageError>;
    fn api_keys_touch(&mut self, id: i32, now: i64) -> Result<(), WalletStorageError>;
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

impl ApiKeyStore for WriteTransaction<'_> {
    fn api_keys_insert(&mut self, name: &str, key_hash: &[u8], permissions: &str) -> Result<ApiKey, WalletStorageError> {
        use crate::schema::api_keys;
        let new_key = NewApiKey {
            name,
            key_hash,
            permissions,
            created_at: now_unix(),
        };
        diesel::insert_into(api_keys::table)
            .values(&new_key)
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("api_keys_insert", e))?;
        api_keys::table
            .order(api_keys::id.desc())
            .first::<ApiKey>(self.connection())
            .map_err(|e| WalletStorageError::general("api_keys_insert_fetch", e))
    }

    fn api_keys_get_by_hash(&mut self, key_hash: &[u8]) -> Result<ApiKey, WalletStorageError> {
        use crate::schema::api_keys;
        api_keys::table
            .filter(api_keys::key_hash.eq(key_hash))
            .first::<ApiKey>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("api_keys_get_by_hash", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "api_keys_get_by_hash",
                entity: "api_key".to_string(),
                key: "<hash>".to_string(),
            })
    }

    fn api_keys_list_all(&mut self) -> Result<Vec<ApiKey>, WalletStorageError> {
        use crate::schema::api_keys;
        api_keys::table
            .order(api_keys::created_at.desc())
            .get_results::<ApiKey>(self.connection())
            .map_err(|e| WalletStorageError::general("api_keys_list_all", e))
    }

    fn api_keys_revoke(&mut self, id: i32, now: i64) -> Result<(), WalletStorageError> {
        use crate::schema::api_keys;
        diesel::update(api_keys::table.filter(api_keys::id.eq(id)))
            .set(api_keys::revoked_at.eq(Some(now)))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("api_keys_revoke", e))?;
        Ok(())
    }

    fn api_keys_touch(&mut self, id: i32, now: i64) -> Result<(), WalletStorageError> {
        use crate::schema::api_keys;
        diesel::update(api_keys::table.filter(api_keys::id.eq(id)))
            .set(api_keys::last_used_at.eq(Some(now)))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("api_keys_touch", e))?;
        Ok(())
    }
}

impl ApiKeyStore for ReadTransaction<'_> {
    fn api_keys_insert(&mut self, _name: &str, _key_hash: &[u8], _permissions: &str) -> Result<ApiKey, WalletStorageError> {
        Err(WalletStorageError::general("api_keys_insert", "write operation on read transaction"))
    }

    fn api_keys_get_by_hash(&mut self, key_hash: &[u8]) -> Result<ApiKey, WalletStorageError> {
        use crate::schema::api_keys;
        api_keys::table
            .filter(api_keys::key_hash.eq(key_hash))
            .first::<ApiKey>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("api_keys_get_by_hash", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "api_keys_get_by_hash",
                entity: "api_key".to_string(),
                key: "<hash>".to_string(),
            })
    }

    fn api_keys_list_all(&mut self) -> Result<Vec<ApiKey>, WalletStorageError> {
        use crate::schema::api_keys;
        api_keys::table
            .order(api_keys::created_at.desc())
            .get_results::<ApiKey>(self.connection())
            .map_err(|e| WalletStorageError::general("api_keys_list_all", e))
    }

    fn api_keys_revoke(&mut self, _id: i32, _now: i64) -> Result<(), WalletStorageError> {
        Err(WalletStorageError::general("api_keys_revoke", "write operation on read transaction"))
    }

    fn api_keys_touch(&mut self, _id: i32, _now: i64) -> Result<(), WalletStorageError> {
        Err(WalletStorageError::general("api_keys_touch", "write operation on read transaction"))
    }
}
