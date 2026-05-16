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

// ─── API-key storage extension ────────────────────────────────────────────────────────────────────────────────
// Keeps api_key CRUD out of the SDK trait layer so no core trait changes needed.

/// Re-export the raw DB row type for callers that need it (e.g. walletd handlers).
pub use models::api_key::ApiKey as ApiKeyRow;

pub trait ApiKeyStore {
    fn api_keys_insert(&mut self, name: &str, key_hash: &[u8], permissions: &str) -> Result<ApiKey, WalletStorageError>;
    fn api_keys_get_by_hash(&mut self, key_hash: &[u8]) -> Result<ApiKey, WalletStorageError>;
    fn api_keys_list_all(&mut self) -> Result<Vec<ApiKey>, WalletStorageError>;
    fn api_keys_revoke(&mut self, id: i64, now: i64) -> Result<(), WalletStorageError>;
    fn api_keys_touch(&mut self, id: i64, now: i64) -> Result<(), WalletStorageError>;
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn api_keys_get_by_hash_conn(conn: &mut SqliteConnection, key_hash: &[u8]) -> Result<ApiKey, WalletStorageError> {
    use crate::schema::api_keys;
    api_keys::table
        .filter(api_keys::key_hash.eq(key_hash))
        .first::<ApiKey>(conn)
        .optional()
        .map_err(|e| WalletStorageError::general("api_keys_get_by_hash", e))?
        .ok_or_else(|| WalletStorageError::NotFound {
            operation: "api_keys_get_by_hash",
            entity: "api_key".to_string(),
            key: "<hash>".to_string(),
        })
}

fn api_keys_list_all_conn(conn: &mut SqliteConnection) -> Result<Vec<ApiKey>, WalletStorageError> {
    use crate::schema::api_keys;
    api_keys::table
        .order(api_keys::created_at.desc())
        .get_results::<ApiKey>(conn)
        .map_err(|e| WalletStorageError::general("api_keys_list_all", e))
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
            .filter(api_keys::key_hash.eq(key_hash))
            .first::<ApiKey>(self.connection())
            .map_err(|e| WalletStorageError::general("api_keys_insert_fetch", e))
    }

    fn api_keys_get_by_hash(&mut self, key_hash: &[u8]) -> Result<ApiKey, WalletStorageError> {
        api_keys_get_by_hash_conn(self.connection(), key_hash)
    }

    fn api_keys_list_all(&mut self) -> Result<Vec<ApiKey>, WalletStorageError> {
        api_keys_list_all_conn(self.connection())
    }

    fn api_keys_revoke(&mut self, id: i64, now: i64) -> Result<(), WalletStorageError> {
        use crate::schema::api_keys;
        // Filter on revoked_at IS NULL so double-revoke is idempotent:
        // the second call affects 0 rows and the original timestamp is preserved.
        diesel::update(api_keys::table.filter(api_keys::id.eq(id).and(api_keys::revoked_at.is_null())))
            .set(api_keys::revoked_at.eq(Some(now)))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("api_keys_revoke", e))?;
        Ok(())
    }

    fn api_keys_touch(&mut self, id: i64, now: i64) -> Result<(), WalletStorageError> {
        use crate::schema::api_keys;
        // Filter on revoked_at IS NULL so a revoked key's last_used_at is
        // never updated by a concurrent in-flight request that raced with revocation.
        diesel::update(api_keys::table.filter(api_keys::id.eq(id).and(api_keys::revoked_at.is_null())))
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
        api_keys_get_by_hash_conn(self.connection(), key_hash)
    }

    fn api_keys_list_all(&mut self) -> Result<Vec<ApiKey>, WalletStorageError> {
        api_keys_list_all_conn(self.connection())
    }

    fn api_keys_revoke(&mut self, _id: i64, _now: i64) -> Result<(), WalletStorageError> {
        Err(WalletStorageError::general("api_keys_revoke", "write operation on read transaction"))
    }

    fn api_keys_touch(&mut self, _id: i64, _now: i64) -> Result<(), WalletStorageError> {
        Err(WalletStorageError::general("api_keys_touch", "write operation on read transaction"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_store() -> SqliteWalletStore {
        let mut conn = SqliteConnection::establish(":memory:").expect("in-memory db");
        sql_query("PRAGMA foreign_keys = ON;")
            .execute(&mut conn)
            .expect("pragma");
        let store = SqliteWalletStore {
            connection: Arc::new(Mutex::new(conn)),
        };
        store.run_migrations().expect("migrations");
        store
    }

    #[test]
    fn api_key_insert_and_get_by_hash() {
        let store = in_memory_store();
        let hash = [0xab_u8; 32];
        let mut tx = store.create_write_tx().unwrap();
        let row = tx.api_keys_insert("my-agent", &hash, "[]").unwrap();
        assert_eq!(row.name, "my-agent");
        assert_eq!(row.key_hash, hash.as_slice());
        assert_eq!(row.permissions, "[]");
        assert!(row.revoked_at.is_none());
        assert!(row.last_used_at.is_none());

        let fetched = tx.api_keys_get_by_hash(&hash).unwrap();
        assert_eq!(fetched.id, row.id);
        assert_eq!(fetched.name, row.name);
    }

    #[test]
    fn api_key_list_all_returns_all_inserted() {
        let store = in_memory_store();
        let mut tx = store.create_write_tx().unwrap();
        tx.api_keys_insert("key-a", &[1u8; 32], "[]").unwrap();
        tx.api_keys_insert("key-b", &[2u8; 32], "[]").unwrap();
        let keys = tx.api_keys_list_all().unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn api_key_get_by_hash_not_found_returns_not_found_error() {
        let store = in_memory_store();
        let mut tx = store.create_write_tx().unwrap();
        let result = tx.api_keys_get_by_hash(&[0u8; 32]);
        assert!(
            matches!(result, Err(WalletStorageError::NotFound { .. })),
            "expected NotFound, got: {:?}",
            result
        );
    }

    #[test]
    fn api_key_revoke_sets_revoked_at_timestamp() {
        let store = in_memory_store();
        let hash = [0xcd_u8; 32];
        let mut tx = store.create_write_tx().unwrap();
        let row = tx.api_keys_insert("revokable", &hash, "[]").unwrap();
        assert!(row.revoked_at.is_none());

        tx.api_keys_revoke(row.id, 1_000_000).unwrap();
        let fetched = tx.api_keys_get_by_hash(&hash).unwrap();
        assert_eq!(fetched.revoked_at, Some(1_000_000));
        assert_eq!(fetched.id, row.id);
    }

    #[test]
    fn api_key_double_revoke_preserves_original_timestamp() {
        let store = in_memory_store();
        let hash = [0xdc_u8; 32];
        let mut tx = store.create_write_tx().unwrap();
        let row = tx.api_keys_insert("double-revoke", &hash, "[]").unwrap();
        tx.api_keys_revoke(row.id, 1_000_000).unwrap();
        // Second revoke with a later timestamp — original must win because
        // the UPDATE filters revoked_at IS NULL.
        tx.api_keys_revoke(row.id, 9_999_999).unwrap();
        let fetched = tx.api_keys_get_by_hash(&hash).unwrap();
        assert_eq!(
            fetched.revoked_at,
            Some(1_000_000),
            "double-revoke must preserve the original timestamp",
        );
    }

    #[test]
    fn api_key_touch_updates_last_used_at() {
        let store = in_memory_store();
        let hash = [0xef_u8; 32];
        let mut tx = store.create_write_tx().unwrap();
        let row = tx.api_keys_insert("touchable", &hash, "[]").unwrap();
        assert!(row.last_used_at.is_none());

        tx.api_keys_touch(row.id, 9_999_999).unwrap();
        let fetched = tx.api_keys_get_by_hash(&hash).unwrap();
        assert_eq!(fetched.last_used_at, Some(9_999_999));
    }

    #[test]
    fn api_key_touch_on_revoked_key_does_not_update_last_used_at() {
        let store = in_memory_store();
        let hash = [0xfe_u8; 32];
        let mut tx = store.create_write_tx().unwrap();
        let row = tx.api_keys_insert("race-target", &hash, "[]").unwrap();
        tx.api_keys_revoke(row.id, 1_000).unwrap();
        // Touch after revoke must not error, but must not update last_used_at.
        tx.api_keys_touch(row.id, 2_000).unwrap();
        let fetched = tx.api_keys_get_by_hash(&hash).unwrap();
        assert!(
            fetched.last_used_at.is_none(),
            "touch on revoked key must not update last_used_at",
        );
    }

    #[test]
    fn api_key_read_tx_write_ops_return_error() {
        let store = in_memory_store();
        let mut rtx = store.create_read_tx().unwrap();
        assert!(
            rtx.api_keys_insert("fail", &[0u8; 32], "[]").is_err(),
            "insert must fail on read tx"
        );
        assert!(
            rtx.api_keys_revoke(1, 0).is_err(),
            "revoke must fail on read tx"
        );
        assert!(
            rtx.api_keys_touch(1, 0).is_err(),
            "touch must fail on read tx"
        );
        let keys = rtx.api_keys_list_all().unwrap();
        assert!(keys.is_empty());
    }
}
