//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Storage-level tests for the `address_book_*` writer/reader methods.
//!
//! These exercise the behaviour the walletd JSON-RPC layer and the web-ui
//! `AddressBookPage` rely on:
//!   * a single `address_book_update` UPDATE statement applies rename + address + memo in one round-trip (previously
//!     three separate UPDATEs),
//!   * passing `memo = Some("")` actually clears the previously-stored memo (the UI relies on this to distinguish
//!     "unchanged" from "cleared"),
//!   * passing `memo = None` leaves the existing memo untouched, and
//!   * inserting or renaming onto a name that already exists surfaces the typed [`WalletStorageError::DuplicateName`]
//!     variant — not a generic `GeneralFailure` wrapping a sqlite "UNIQUE constraint failed" string.

use tari_ootle_wallet_sdk::storage::{
    CommittableStore,
    WalletStorageError,
    WalletStoreReader,
    WalletStoreWriter,
    WriteableWalletStore,
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;

fn open_store() -> SqliteWalletStore {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    db
}

#[test]
fn insert_and_get_round_trips() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    let inserted = tx
        .address_book_insert("alice", "otl_loc_alice", Some("work wallet"))
        .unwrap();
    tx.commit().unwrap();

    assert_eq!(inserted.name, "alice");
    assert_eq!(inserted.address, "otl_loc_alice");
    assert_eq!(inserted.memo.as_deref(), Some("work wallet"));

    let mut tx = db.create_write_tx().unwrap();
    let fetched = tx.address_book_get("alice").unwrap();
    assert_eq!(fetched.address, "otl_loc_alice");
    assert_eq!(fetched.memo.as_deref(), Some("work wallet"));
}

#[test]
fn update_rename_and_fields_in_single_call() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    tx.address_book_insert("alice", "otl_loc_alice", Some("old memo"))
        .unwrap();
    tx.commit().unwrap();

    // Rename + change address + change memo should all be applied by the
    // single consolidated UPDATE statement.
    let mut tx = db.create_write_tx().unwrap();
    let updated = tx
        .address_book_update("alice", Some("alice_v2"), Some("otl_loc_alice_v2"), Some("new memo"))
        .unwrap();
    tx.commit().unwrap();

    assert_eq!(updated.name, "alice_v2");
    assert_eq!(updated.address, "otl_loc_alice_v2");
    assert_eq!(updated.memo.as_deref(), Some("new memo"));

    // The row is now only reachable under the new name.
    let mut tx = db.create_write_tx().unwrap();
    assert!(tx.address_book_get("alice").is_err());
    let fetched = tx.address_book_get("alice_v2").unwrap();
    assert_eq!(fetched.address, "otl_loc_alice_v2");
    assert_eq!(fetched.memo.as_deref(), Some("new memo"));
}

#[test]
fn update_with_none_leaves_fields_untouched() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    tx.address_book_insert("bob", "otl_loc_bob", Some("keep me")).unwrap();
    tx.commit().unwrap();

    // All fields `None` — only `updated_at` should be bumped.
    let mut tx = db.create_write_tx().unwrap();
    let updated = tx.address_book_update("bob", None, None, None).unwrap();
    tx.commit().unwrap();

    assert_eq!(updated.name, "bob");
    assert_eq!(updated.address, "otl_loc_bob");
    assert_eq!(updated.memo.as_deref(), Some("keep me"));
}

#[test]
fn update_with_empty_memo_clears_the_memo() {
    // Regression test for the UI bug where clearing a memo silently did
    // nothing: the walletd client serialises an empty memo string for
    // "user cleared the field", and that must actually overwrite the
    // previously-stored memo at the SQL layer.
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    tx.address_book_insert("carol", "otl_loc_carol", Some("to be cleared"))
        .unwrap();
    tx.commit().unwrap();

    let mut tx = db.create_write_tx().unwrap();
    let updated = tx.address_book_update("carol", None, None, Some("")).unwrap();
    tx.commit().unwrap();

    assert_eq!(
        updated.memo.as_deref(),
        Some(""),
        "empty memo string must overwrite the stored memo",
    );
}

#[test]
fn insert_duplicate_name_returns_duplicate_name_variant() {
    // The UI relies on the typed `DuplicateName` variant to render a
    // user-friendly error without string-matching sqlite's driver text.
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    tx.address_book_insert("dave", "otl_loc_dave", None).unwrap();
    tx.commit().unwrap();

    let mut tx = db.create_write_tx().unwrap();
    let err = tx.address_book_insert("dave", "otl_loc_dave_second", None).unwrap_err();
    assert!(
        matches!(err, WalletStorageError::DuplicateName { ref name } if name == "dave"),
        "expected DuplicateName {{ name: \"dave\" }}, got {err:?}",
    );
}

#[test]
fn rename_onto_existing_name_returns_duplicate_name_variant() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    tx.address_book_insert("eve", "otl_loc_eve", None).unwrap();
    tx.address_book_insert("frank", "otl_loc_frank", None).unwrap();
    tx.commit().unwrap();

    // Renaming "frank" -> "eve" must violate the UNIQUE constraint and be
    // surfaced as DuplicateName { name: "eve" } so the UI can show a
    // message scoped to the attempted new name.
    let mut tx = db.create_write_tx().unwrap();
    let err = tx.address_book_update("frank", Some("eve"), None, None).unwrap_err();
    assert!(
        matches!(err, WalletStorageError::DuplicateName { ref name } if name == "eve"),
        "expected DuplicateName {{ name: \"eve\" }}, got {err:?}",
    );
}

#[test]
fn update_missing_entry_returns_not_found() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    let err = tx.address_book_update("ghost", Some("ghost2"), None, None).unwrap_err();
    assert!(
        matches!(err, WalletStorageError::NotFound { .. }),
        "expected NotFound, got {err:?}",
    );
}

#[test]
fn delete_round_trips() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    tx.address_book_insert("heidi", "otl_loc_heidi", None).unwrap();
    tx.commit().unwrap();

    let mut tx = db.create_write_tx().unwrap();
    tx.address_book_delete("heidi").unwrap();
    tx.commit().unwrap();

    let mut tx = db.create_write_tx().unwrap();
    assert!(tx.address_book_get("heidi").is_err());
}
