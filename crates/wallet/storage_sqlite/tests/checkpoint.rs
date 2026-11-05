// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::fs;

use diesel::{Connection, RunQueryDsl, SqliteConnection, sql_query};
use tari_ootle_wallet_sdk::storage::{WalletStoreWriter, WriteableWalletStore};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;

#[test]
fn a_checkpoint_empties_the_write_ahead_log_into_the_database() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("wallet.sqlite");
    let store = SqliteWalletStore::try_open(&path).unwrap();
    store.run_migrations().unwrap();
    let wal = temp.path().join("wallet.sqlite-wal");
    let database_len = fs::metadata(&path).unwrap().len();
    assert!(fs::metadata(&wal).unwrap().len() > 0, "the migrations are in the log");

    store.checkpoint().unwrap();

    assert_eq!(fs::metadata(&wal).unwrap().len(), 0);
    assert!(fs::metadata(&path).unwrap().len() > database_len);
}

#[test]
fn a_checkpoint_that_a_reader_blocks_fails_and_keeps_the_log() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("wallet.sqlite");
    let store = SqliteWalletStore::try_open(&path).unwrap();
    store.run_migrations().unwrap();
    store
        .with_write_tx(|tx| tx.config_set("written", &1u64, false))
        .unwrap();
    let wal = temp.path().join("wallet.sqlite-wal");
    let wal_len = fs::metadata(&wal).unwrap().len();

    let mut reader = SqliteConnection::establish(path.to_str().unwrap()).unwrap();
    sql_query("BEGIN").execute(&mut reader).unwrap();
    sql_query("SELECT * FROM config").execute(&mut reader).unwrap();

    assert!(store.checkpoint().is_err());
    assert_eq!(fs::metadata(&wal).unwrap().len(), wal_len);
}
