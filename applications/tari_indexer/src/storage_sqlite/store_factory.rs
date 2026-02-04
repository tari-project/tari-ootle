//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::Debug,
    fs::create_dir_all,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use diesel::{Connection, RunQueryDsl, SqliteConnection, sql_query};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness};
use tari_ootle_storage::StorageError;
use tari_ootle_storage_sqlite::{SqliteTransaction, error::SqliteStorageError};

use crate::{
    storage_sqlite::{reader::SqliteStoreReadTransaction, writer::SqliteStoreWriteTransaction},
    store::{IndexerStore, IndexerStoreReader},
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
                operation: "set pragma",
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

impl IndexerStoreReader for SqliteIndexerStore {
    type ReadTransaction<'a> = SqliteStoreReadTransaction<'a>;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, StorageError> {
        let tx = SqliteTransaction::begin(self.connection.lock().unwrap())?;
        Ok(SqliteStoreReadTransaction::new(tx))
    }
}

impl IndexerStore for SqliteIndexerStore {
    type WriteTransaction<'a> = SqliteStoreWriteTransaction<'a>;

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, StorageError> {
        let tx = SqliteTransaction::begin(self.connection.lock().unwrap())?;
        Ok(SqliteStoreWriteTransaction::new(tx))
    }
}
