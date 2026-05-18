//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Debug, fs::create_dir_all, path::PathBuf, time::Duration};

use async_trait::async_trait;
use deadpool_diesel::{
    Runtime,
    sqlite::{Hook, HookError, Manager, Pool},
};
use diesel::{Connection, RunQueryDsl, SqliteConnection, sql_query};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness};
use tari_ootle_storage::StorageError;
use tari_ootle_storage_sqlite::{SqliteTransaction, error::SqliteStorageError};

use crate::{
    storage_sqlite::{reader::SqliteStoreReadTransaction, writer::SqliteStoreWriteTransaction},
    store::{IndexerStore, IndexerStoreReader, IndexerStoreWriteTransaction},
};

const LOG_TARGET: &str = "tari::indexer::storage_sqlite";
const POOL_MAX_SIZE: usize = 16;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct SqliteIndexerStore {
    pool: Pool,
}

impl SqliteIndexerStore {
    pub fn try_create(path: PathBuf) -> Result<Self, StorageError> {
        create_dir_all(path.parent().unwrap()).map_err(|_| StorageError::FileSystemPathDoesNotExist)?;

        let database_url = path.to_str().expect("database_url utf-8 error").to_string();

        // Run migrations on a one-shot connection before opening the pool, so pooled connections
        // never observe a partially-migrated schema.
        let mut migration_conn = SqliteConnection::establish(&database_url).map_err(SqliteStorageError::from)?;
        apply_pragmas(&mut migration_conn).map_err(|source| SqliteStorageError::DieselError {
            source,
            operation: "set pragma",
        })?;
        pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./src/storage_sqlite/migrations");
        if let Err(err) = migration_conn.run_pending_migrations(MIGRATIONS) {
            log::error!(target: LOG_TARGET, "Error running migrations: {}", err);
        }
        drop(migration_conn);

        let manager = Manager::new(database_url, Runtime::Tokio1);
        let pool = Pool::builder(manager)
            .max_size(POOL_MAX_SIZE)
            .post_create(Hook::async_fn(|conn, _metrics| {
                Box::pin(async move {
                    conn.interact(apply_pragmas)
                        .await
                        .map_err(|e| HookError::message(format!("post_create panicked: {e}")))?
                        .map_err(|e| HookError::message(format!("apply_pragmas failed: {e}")))?;
                    Ok(())
                })
            }))
            .build()
            .map_err(|e| StorageError::General {
                details: format!("Failed to build sqlite connection pool: {}", e),
            })?;

        Ok(Self { pool })
    }

    async fn acquire(&self) -> Result<deadpool_diesel::sqlite::Connection, StorageError> {
        self.pool.get().await.map_err(|e| StorageError::General {
            details: format!("Failed to acquire sqlite connection from pool: {}", e),
        })
    }
}

impl Debug for SqliteIndexerStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SqliteIndexerStore {{ pool: ... }}")
    }
}

#[async_trait]
impl IndexerStoreReader for SqliteIndexerStore {
    type ReadTransaction<'a> = SqliteStoreReadTransaction<'a>;

    async fn with_read_tx<F, R, E>(&self, f: F) -> Result<R, E>
    where
        F: for<'a> FnOnce(&mut Self::ReadTransaction<'a>) -> Result<R, E> + Send + 'static,
        R: Send + 'static,
        E: From<StorageError> + Send + 'static,
    {
        let conn = self.acquire().await?;
        let result: Result<R, E> = conn
            .interact(move |c| -> Result<R, E> {
                let inner = SqliteTransaction::begin(c)
                    .map_err(StorageError::from)
                    .map_err(E::from)?;
                let mut tx = SqliteStoreReadTransaction::new(inner);
                f(&mut tx)
            })
            .await
            .map_err(|e| StorageError::General {
                details: format!("Pool interact panicked: {}", e),
            })?;
        result
    }
}

#[async_trait]
impl IndexerStore for SqliteIndexerStore {
    type WriteTransaction<'a> = SqliteStoreWriteTransaction<'a>;

    async fn with_write_tx<F, R, E>(&self, f: F) -> Result<R, E>
    where
        F: for<'a> FnOnce(&mut Self::WriteTransaction<'a>) -> Result<R, E> + Send + 'static,
        R: Send + 'static,
        E: From<StorageError> + Send + 'static,
    {
        let conn = self.acquire().await?;
        let result: Result<R, E> = conn
            .interact(move |c| -> Result<R, E> {
                let inner = SqliteTransaction::begin_immediate(c)
                    .map_err(StorageError::from)
                    .map_err(E::from)?;
                let mut tx = SqliteStoreWriteTransaction::new(inner);
                match f(&mut tx) {
                    Ok(r) => {
                        tx.commit().map_err(E::from)?;
                        Ok(r)
                    },
                    Err(e) => {
                        if let Err(err) = tx.rollback() {
                            log::error!(target: LOG_TARGET, "Failed to rollback transaction: {}", err);
                        }
                        Err(e)
                    },
                }
            })
            .await
            .map_err(|e| StorageError::General {
                details: format!("Pool interact panicked: {}", e),
            })?;
        result
    }
}

fn apply_pragmas(conn: &mut SqliteConnection) -> Result<(), diesel::result::Error> {
    let busy_timeout_ms = BUSY_TIMEOUT.as_millis();
    sql_query("PRAGMA journal_mode = WAL;").execute(conn)?;
    sql_query("PRAGMA synchronous = NORMAL;").execute(conn)?;
    sql_query("PRAGMA foreign_keys = ON;").execute(conn)?;
    sql_query(format!("PRAGMA busy_timeout = {};", busy_timeout_ms)).execute(conn)?;
    Ok(())
}
