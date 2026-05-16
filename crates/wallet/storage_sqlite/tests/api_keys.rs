//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Storage-layer integration tests for `ApiKeyStore`.
//!
//! These tests compile as a separate crate (Rust integration-test convention)
//! and exercise only the public API of `tari_ootle_wallet_storage_sqlite`.
//! They verify the invariants the walletd auth handlers rely on:
//!
//!   * insert → get_by_hash round-trips with correct field values
//!   * unknown hash → `NotFound` (no enumeration of valid hashes)
//!   * revocation is a soft-delete: row survives with `revoked_at` set
//!   * double-revoke is idempotent: first timestamp is preserved
//!   * touch updates `last_used_at` for active keys
//!   * touch on a revoked key is a silent no-op (race safety)
//!   * `list_all` returns both active and revoked rows (audit trail)
//!   * duplicate `key_hash` is rejected (UNIQUE constraint)

use tari_ootle_wallet_sdk::storage::{CommittableStore, WalletStorageError, WriteableWalletStore};
use tari_ootle_wallet_storage_sqlite::{ApiKeyStore, SqliteWalletStore};

fn open_store() -> SqliteWalletStore {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    db
}

#[test]
fn insert_and_get_by_hash_round_trips() {
    let db = open_store();
    let hash = [0xaa_u8; 32];

    let inserted = {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx.api_keys_insert("my-agent", &hash, r#"["AccountList"]"#).unwrap();
        tx.commit().unwrap();
        row
    };

    assert_eq!(inserted.name, "my-agent");
    assert_eq!(inserted.key_hash.as_slice(), &hash);
    assert_eq!(inserted.permissions, r#"["AccountList"]"#);
    assert!(inserted.revoked_at.is_none());
    assert!(inserted.last_used_at.is_none());

    let mut tx = db.create_write_tx().unwrap();
    let fetched = tx.api_keys_get_by_hash(&hash).unwrap();
    assert_eq!(fetched.id, inserted.id);
    assert_eq!(fetched.name, "my-agent");
}

#[test]
fn get_by_hash_unknown_hash_returns_not_found() {
    // Callers must not be able to distinguish "no such key" from "revoked key"
    // via the error type — both surface as NotFound at the auth boundary.
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    let result = tx.api_keys_get_by_hash(&[0xff_u8; 32]);
    assert!(
        matches!(result, Err(WalletStorageError::NotFound { .. })),
        "expected NotFound for unknown hash, got: {result:?}",
    );
}

#[test]
fn revoke_is_soft_delete_key_remains_retrievable() {
    // Revocation must not remove the row — admins need the audit trail.
    // The auth handler applies the revoked_at IS NULL filter itself.
    let db = open_store();
    let hash = [0xbb_u8; 32];
    let now = 1_700_000_000_i64;

    let row_id = {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx.api_keys_insert("revokable", &hash, "[]").unwrap();
        tx.commit().unwrap();
        row.id
    };

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_keys_revoke(row_id, now).unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let fetched = tx.api_keys_get_by_hash(&hash).unwrap();
    assert_eq!(
        fetched.revoked_at,
        Some(now),
        "revoked_at must be set to the provided timestamp",
    );
    assert_eq!(fetched.id, row_id, "soft-delete: row must still be retrievable");
}

#[test]
fn double_revoke_preserves_original_timestamp() {
    // If two callers race to revoke the same key, the first timestamp wins.
    // The second UPDATE targets revoked_at IS NULL so it matches 0 rows.
    let db = open_store();
    let hash = [0xcc_u8; 32];
    let first_ts = 1_000_000_i64;
    let second_ts = 9_000_000_i64;

    let row_id = {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx.api_keys_insert("double-revoke", &hash, "[]").unwrap();
        tx.commit().unwrap();
        row.id
    };

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_keys_revoke(row_id, first_ts).unwrap();
        tx.commit().unwrap();
    }
    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_keys_revoke(row_id, second_ts).unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let fetched = tx.api_keys_get_by_hash(&hash).unwrap();
    assert_eq!(
        fetched.revoked_at,
        Some(first_ts),
        "double-revoke must preserve the original revoked_at timestamp",
    );
}

#[test]
fn touch_updates_last_used_at_for_active_key() {
    let db = open_store();
    let hash = [0xdd_u8; 32];
    let ts = 9_000_000_i64;

    let row_id = {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx.api_keys_insert("touchable", &hash, "[]").unwrap();
        tx.commit().unwrap();
        row.id
    };

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_keys_touch(row_id, ts).unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let fetched = tx.api_keys_get_by_hash(&hash).unwrap();
    assert_eq!(fetched.last_used_at, Some(ts));
}

#[test]
fn touch_on_revoked_key_is_noop_not_error() {
    // An auth request may race with a concurrent revocation: the in-flight
    // request passes the active-key check but the touch happens after revoke.
    // The touch must silently skip the update (0 rows affected) rather than
    // erroring, and must not corrupt the revoked_at timestamp.
    let db = open_store();
    let hash = [0xee_u8; 32];

    let row_id = {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx.api_keys_insert("race-target", &hash, "[]").unwrap();
        tx.commit().unwrap();
        row.id
    };

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_keys_revoke(row_id, 1_000).unwrap();
        tx.commit().unwrap();
    }

    {
        let mut tx = db.create_write_tx().unwrap();
        // Must not error.
        tx.api_keys_touch(row_id, 2_000).unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let fetched = tx.api_keys_get_by_hash(&hash).unwrap();
    assert!(
        fetched.last_used_at.is_none(),
        "touch on a revoked key must not update last_used_at",
    );
    assert_eq!(
        fetched.revoked_at,
        Some(1_000),
        "touch must not corrupt revoked_at",
    );
}

#[test]
fn list_all_includes_active_and_revoked_rows() {
    // list_all is the admin audit view — it must return every key regardless
    // of revocation status so admins can review the full credential history.
    let db = open_store();
    let now = 1_700_000_000_i64;

    let (id_a, id_b, id_c) = {
        let mut tx = db.create_write_tx().unwrap();
        let a = tx.api_keys_insert("alpha", &[0x11_u8; 32], "[]").unwrap();
        let b = tx.api_keys_insert("beta",  &[0x22_u8; 32], "[]").unwrap();
        let c = tx.api_keys_insert("gamma", &[0x33_u8; 32], "[]").unwrap();
        tx.commit().unwrap();
        (a.id, b.id, c.id)
    };

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_keys_revoke(id_b, now).unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let keys = tx.api_keys_list_all().unwrap();
    assert_eq!(keys.len(), 3, "list_all must include both active and revoked rows");

    let beta = keys.iter().find(|k| k.id == id_b).expect("beta must be in list");
    assert!(beta.revoked_at.is_some(), "beta must appear as revoked in list_all");

    let alpha = keys.iter().find(|k| k.id == id_a).expect("alpha must be in list");
    let gamma = keys.iter().find(|k| k.id == id_c).expect("gamma must be in list");
    assert!(alpha.revoked_at.is_none(), "alpha must appear as active");
    assert!(gamma.revoked_at.is_none(), "gamma must appear as active");
}

#[test]
fn insert_duplicate_key_hash_returns_error() {
    // key_hash has a UNIQUE constraint — inserting two keys with the same
    // hash must fail so a collision can't silently shadow a live credential.
    let db = open_store();
    let hash = [0xff_u8; 32];

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_keys_insert("original", &hash, "[]").unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let result = tx.api_keys_insert("duplicate", &hash, "[]");
    assert!(
        result.is_err(),
        "inserting a key with a duplicate hash must return an error",
    );
}
