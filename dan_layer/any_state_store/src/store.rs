//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use tari_dan_common_types::PeerAddress;
use tari_dan_storage::{StateStore, StorageError};
use tari_state_store_rocksdb::RocksDbStateStore;
use tari_state_store_sqlite::SqliteStateStore;

use crate::{
    config::AnyDatabaseType,
    reader::AnyStateStoreReadTransaction,
    writer::AnyStateStoreWriteTransaction,
    Config,
};

#[derive(Debug, Clone)]
pub enum AnyStateStore {
    Rocksdb(RocksDbStateStore<PeerAddress>),
    Sqlite(SqliteStateStore<PeerAddress>),
}

impl AnyStateStore {
    pub fn connect(config: &Config) -> Result<Self, anyhow::Error> {
        match config.database_type {
            AnyDatabaseType::Rocksdb => {
                let db: RocksDbStateStore<PeerAddress> = RocksDbStateStore::connect(&config.rocks_db.path)?;
                Ok(Self::Rocksdb(db))
            },
            AnyDatabaseType::Sqlite => {
                let sqlite_connection_str = config.sqlite.to_connection_string();
                let db: SqliteStateStore<PeerAddress> = SqliteStateStore::connect(&sqlite_connection_str)?;
                Ok(Self::Sqlite(db))
            },
        }
    }
}

impl StateStore for AnyStateStore {
    type Addr = PeerAddress;
    type ReadTransaction<'a> = AnyStateStoreReadTransaction<'a>;
    type WriteTransaction<'a> = AnyStateStoreWriteTransaction<'a>;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, StorageError> {
        match self {
            AnyStateStore::Rocksdb(db) => {
                let tx = db.create_read_tx()?;
                Ok(AnyStateStoreReadTransaction::Rocksdb(tx))
            },
            AnyStateStore::Sqlite(db) => {
                let tx = db.create_read_tx()?;
                Ok(AnyStateStoreReadTransaction::Sqlite(tx))
            },
        }
    }

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, StorageError> {
        match self {
            AnyStateStore::Rocksdb(db) => {
                let write_tx = db.create_write_tx()?;
                Ok(AnyStateStoreWriteTransaction::Rocksdb {
                    read_tx: AnyStateStoreReadTransaction::RocksdbRef(write_tx.deref()),
                    write_tx,
                })
            },
            AnyStateStore::Sqlite(db) => {
                let write_tx = db.create_write_tx()?;
                Ok(AnyStateStoreWriteTransaction::Sqlite {
                    read_tx: AnyStateStoreReadTransaction::SqliteRef(write_tx.deref()),
                    write_tx,
                })
            },
        }
    }
}
