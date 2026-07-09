//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::optional::Optional;
use tari_ootle_wallet_sdk::{
    models::KeyType,
    storage::{CommittableStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
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

#[test]
fn import_key_is_idempotent_on_public_key() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let mut tx = db.create_write_tx().unwrap();

    let public_key = "aa".repeat(32);
    let id1 = tx
        .key_manager_insert_imported_key("first-label", &public_key, &[1, 2, 3], KeyType::ViewOnly)
        .unwrap();

    // Re-importing the same key (same public key) returns the same id instead of hitting the unique index, and
    // overwrites the mutable columns (label, encrypted secret, key type).
    let id2 = tx
        .key_manager_insert_imported_key("second-label", &public_key, &[4, 5, 6], KeyType::GeneralPurpose)
        .unwrap();
    assert_eq!(id1, id2);

    let (key_type, encrypted_secret) = tx.key_manager_get_raw_imported_key(id1).unwrap();
    assert_eq!(key_type, KeyType::GeneralPurpose);
    assert_eq!(&*encrypted_secret, &[4, 5, 6]);

    // A distinct public key gets a distinct id.
    let other_public_key = "bb".repeat(32);
    let id3 = tx
        .key_manager_insert_imported_key("other", &other_public_key, &[9], KeyType::ViewOnly)
        .unwrap();
    assert_ne!(id1, id3);

    tx.commit().unwrap();
}
