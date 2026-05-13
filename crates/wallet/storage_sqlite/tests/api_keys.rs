//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::storage::{
    CommittableStore, WalletStorageError, WalletStoreReader, WalletStoreWriter, WriteableWalletStore,
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;

fn open_store() -> SqliteWalletStore {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    db
}

#[test]
fn create_and_find_active_api_key_by_hash() {
    let db = open_store();

    let created = {
        let mut tx = db.create_write_tx().unwrap();
        let api_key = tx
            .api_keys_create("agent", "hash:agent-secret", "wallet:read,wallet:write")
            .unwrap();
        tx.commit().unwrap();
        api_key
    };

    assert!(created.id > 0);
    assert_eq!(created.name, "agent");
    assert_eq!(created.key_hash, "hash:agent-secret");
    assert_eq!(created.permissions, "wallet:read,wallet:write");
    assert!(created.last_used_at.is_none());
    assert!(created.expires_at.is_none());
    assert!(created.revoked_at.is_none());

    let mut tx = db.create_write_tx().unwrap();
    let found = tx.api_keys_find_active_by_hash("hash:agent-secret").unwrap();
    assert_eq!(found.id, created.id);
    assert_eq!(found.key_hash, "hash:agent-secret");

    let by_id = tx.api_keys_find_active_by_id(created.id).unwrap();
    assert_eq!(by_id.id, created.id);
}

#[test]
fn list_api_keys_returns_created_keys() {
    let db = open_store();

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_keys_create("first", "hash:first", "wallet:read").unwrap();
        tx.api_keys_create("second", "hash:second", "wallet:write").unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let keys = tx.api_keys_list().unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].name, "first");
    assert_eq!(keys[1].name, "second");
}

#[test]
fn revoke_hides_key_from_active_lookup_and_list() {
    let db = open_store();

    let id = {
        let mut tx = db.create_write_tx().unwrap();
        let api_key = tx.api_keys_create("agent", "hash:agent", "wallet:read").unwrap();
        tx.commit().unwrap();
        api_key.id
    };

    {
        let mut tx = db.create_write_tx().unwrap();
        let revoked = tx.api_keys_revoke(id).unwrap();
        tx.commit().unwrap();
        assert!(revoked.revoked_at.is_some());
    }

    let mut tx = db.create_write_tx().unwrap();
    let err = tx.api_keys_find_active_by_hash("hash:agent").unwrap_err();
    assert!(
        matches!(err, WalletStorageError::NotFound { .. }),
        "expected revoked key to be hidden from active lookup, got {err:?}",
    );

    let listed = tx.api_keys_list().unwrap();
    assert!(listed.is_empty());

    let err = tx.api_keys_find_active_by_id(id).unwrap_err();
    assert!(
        matches!(err, WalletStorageError::NotFound { .. }),
        "expected revoked key to be hidden from active id lookup, got {err:?}",
    );
}

#[test]
fn touch_updates_last_used_for_active_key() {
    let db = open_store();

    let id = {
        let mut tx = db.create_write_tx().unwrap();
        let api_key = tx.api_keys_create("agent", "hash:agent", "wallet:read").unwrap();
        tx.commit().unwrap();
        api_key.id
    };

    {
        let mut tx = db.create_write_tx().unwrap();
        let touched = tx.api_keys_touch_last_used(id).unwrap();
        tx.commit().unwrap();
        assert!(touched.last_used_at.is_some());
        assert!(touched.expires_at.is_none());
        assert!(touched.revoked_at.is_none());
    }

    let mut tx = db.create_write_tx().unwrap();
    let active = tx.api_keys_find_active_by_hash("hash:agent").unwrap();
    assert!(active.last_used_at.is_some());
}

#[test]
fn revoked_api_key_is_not_touchable() {
    let db = open_store();

    let id = {
        let mut tx = db.create_write_tx().unwrap();
        let api_key = tx.api_keys_create("agent", "hash:agent", "wallet:read").unwrap();
        let revoked = tx.api_keys_revoke(api_key.id).unwrap();
        tx.commit().unwrap();
        assert!(revoked.revoked_at.is_some());
        api_key.id
    };

    let mut tx = db.create_write_tx().unwrap();
    let err = tx.api_keys_touch_last_used(id).unwrap_err();
    assert!(
        matches!(err, WalletStorageError::NotFound { .. }),
        "expected revoked key to be untouchable, got {err:?}",
    );
}

#[test]
fn duplicate_hash_is_rejected() {
    let db = open_store();

    let mut tx = db.create_write_tx().unwrap();
    tx.api_keys_create("first", "hash:duplicate", "wallet:read").unwrap();
    let err = tx
        .api_keys_create("second", "hash:duplicate", "wallet:write")
        .unwrap_err();
    assert!(
        matches!(err, WalletStorageError::GeneralFailure { .. }),
        "expected duplicate hash insert to fail, got {err:?}",
    );
}
