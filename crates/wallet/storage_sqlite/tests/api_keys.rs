// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::{
    models::NewApiKeyModel,
    storage::{CommittableStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;

#[test]
fn api_keys_can_be_created_listed_touched_and_revoked() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let mut tx = db.create_write_tx().unwrap();

    tx.api_keys_insert(NewApiKeyModel {
        id: "key-1".to_string(),
        name: "agent".to_string(),
        key_hash: vec![1; 32],
        permissions: vec!["AccountInfo".to_string(), "TransactionGet".to_string()],
    })
    .unwrap();

    let key = tx.api_keys_get_active("key-1").unwrap();
    assert_eq!(key.name, "agent");
    assert_eq!(key.permissions, vec!["AccountInfo", "TransactionGet"]);
    assert!(key.last_used_at.is_none());

    tx.api_keys_touch_last_used("key-1").unwrap();
    assert!(tx.api_keys_get_active("key-1").unwrap().last_used_at.is_some());
    assert_eq!(tx.api_keys_list_active().unwrap().len(), 1);

    tx.api_keys_revoke("key-1").unwrap();
    assert!(tx.api_keys_get_active("key-1").is_err());
    assert_eq!(tx.api_keys_list_active().unwrap().len(), 0);

    tx.commit().unwrap();
}
