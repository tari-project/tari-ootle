// Copyright 2022 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause
#[macro_use]
extern crate diesel;

mod database_file;
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
};

use diesel::{Connection, RunQueryDsl, SqliteConnection, sql_query};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use tari_ootle_wallet_sdk::storage::{ReadableWalletStore, WalletStorageError, WriteableWalletStore};

pub use crate::database_file::move_database;
use crate::{reader::ReadTransaction, writer::WriteTransaction};

#[derive(Clone)]
pub struct SqliteWalletStore {
    // MUTEX: required to make Sync
    connection: Arc<Mutex<SqliteConnection>>,
}

impl SqliteWalletStore {
    pub fn try_open<P: AsRef<Path>>(path: P) -> Result<Self, WalletStorageError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            create_dir_all(parent).map_err(|e| WalletStorageError::general("create_dir_all", e))?;
        }

        let database_url = path
            .to_str()
            .ok_or_else(|| WalletStorageError::general("try_open", "database path is not valid UTF-8"))?;
        let mut connection =
            SqliteConnection::establish(database_url).map_err(|e| WalletStorageError::general("connect", e))?;

        // WAL with synchronous = FULL keeps every commit durable across power loss at one fsync of the log per commit.
        for pragma in [
            "PRAGMA foreign_keys = ON;",
            "PRAGMA journal_mode = WAL;",
            "PRAGMA synchronous = FULL;",
        ] {
            sql_query(pragma)
                .execute(&mut connection)
                .map_err(|source| WalletStorageError::general("set pragma", source))?;
        }

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

    /// Copies every commit in the write-ahead log into the database file and empties the log, so that the database
    /// file alone holds the wallet. Fails if another connection to the database, such as a `sqlite3` shell, keeps the
    /// checkpoint from completing.
    pub fn checkpoint(&self) -> Result<(), WalletStorageError> {
        #[derive(QueryableByName)]
        struct WalCheckpoint {
            #[diesel(sql_type = diesel::sql_types::Integer)]
            busy: i32,
        }

        let mut conn = self.connection.lock().unwrap();
        let result = sql_query("PRAGMA wal_checkpoint(TRUNCATE);")
            .get_result::<WalCheckpoint>(&mut *conn)
            .map_err(|source| WalletStorageError::general("checkpoint", source))?;
        if result.busy != 0 {
            return Err(WalletStorageError::general(
                "checkpoint",
                "another connection to the database kept the checkpoint from completing",
            ));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_open_handles_memory_path_and_invalid_parent_without_panicking() {
        let store = SqliteWalletStore::try_open(":memory:").unwrap();
        store.run_migrations().unwrap();

        let err = SqliteWalletStore::try_open("/dev/null/impossible/wallet.sqlite").unwrap_err();
        assert!(matches!(err, WalletStorageError::GeneralFailure { .. }));
    }
}
