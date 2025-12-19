//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::optional::Optional;
use tari_ootle_wallet_sdk::storage::{CommittableStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;

#[test]
fn get_and_set_branch_index() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let mut tx = db.create_write_tx().unwrap();
    let index = tx.key_manager_get_active_index("").optional().unwrap();
    assert!(index.is_none());
    tx.key_manager_insert_or_ignore("", 123).unwrap();
    let index = tx.key_manager_get_active_index("").unwrap();
    assert_eq!(index, 123);
    tx.key_manager_insert_or_ignore("another", 1).unwrap();
    tx.key_manager_insert_or_ignore("another", 2).unwrap();
    let index = tx.key_manager_get_active_index("another").unwrap();
    assert_eq!(index, 1);
    tx.key_manager_set_active_index("another", 2).unwrap();
    tx.commit().unwrap();

    let index = tx.key_manager_get_active_index("").unwrap();
    assert_eq!(index, 123);
    let index = tx.key_manager_get_active_index("another").unwrap();
    assert_eq!(index, 2);
}
