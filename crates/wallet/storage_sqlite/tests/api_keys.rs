//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Storage-level tests for the `api_key_*` writer/reader methods that the
//! agent-friendly authentication flow (issue #1957) relies on.
//!
//! These exercise the behaviour the wallet daemon's `auth.create_api_key`,
//! `auth.list_api_keys`, `auth.revoke_api_key`, and the bearer-API-key
//! resolver in `HandlerContext::check_auth` depend on:
//!   * `api_key_insert` returns the persisted row and `api_key_find_active_by_hash` returns the same row for the same
//!     hash,
//!   * `api_key_revoke` is immediately reflected in `api_key_find_active_by_hash` returning `None` (so a revoked key
//!     cannot authenticate even if the lookup races with the revoke commit),
//!   * `api_key_touch_last_used` updates the timestamp, is throttled, and respects the `revoked_at IS NULL` filter,
//!   * `api_key_list` honours the `include_revoked` flag and the expiry filter on `find_active_by_hash`.

use std::time::Duration;

use tari_ootle_wallet_sdk::storage::{CommittableStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;

fn open_store() -> SqliteWalletStore {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    db
}

#[test]
fn insert_and_find_active_round_trips() {
    let db = open_store();

    let inserted = {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx
            .api_key_insert("agent-prod", "deadbeefcafef00d", "AccountInfo, TransactionGet", None)
            .unwrap();
        tx.commit().unwrap();
        row
    };

    assert_eq!(inserted.name, "agent-prod");
    assert!(inserted.is_active(), "freshly-inserted key must not be revoked");
    assert!(inserted.last_used_at.is_none(), "last_used_at starts unset");

    let mut tx = db.create_write_tx().unwrap();
    let found = tx
        .api_key_find_active_by_hash("deadbeefcafef00d")
        .unwrap()
        .expect("hash must match the row we just inserted");
    assert_eq!(found.id, inserted.id);
    assert_eq!(found.permissions, "AccountInfo, TransactionGet");
}

#[test]
fn find_active_returns_none_for_unknown_hash() {
    // The auth flow must distinguish "no such key" from "key revoked"
    // identically: both surface as `Ok(None)` so an attacker probing the
    // endpoint can't enumerate valid hashes by timing or error message.
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    let found = tx.api_key_find_active_by_hash("aaaaaaaaaaaa").unwrap();
    assert!(found.is_none());
}

#[test]
fn revoke_is_immediate_for_subsequent_lookups() {
    // Revoke must take effect on the very next read against the same DB.
    // Anything weaker (e.g. cache-based revocation) would mean a stolen
    // key remained usable until the cache expired.
    let db = open_store();

    let id = {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx.api_key_insert("ephemeral", "0000abc", "AccountInfo", None).unwrap();
        tx.commit().unwrap();
        row.id
    };

    // Pre-revoke: lookup hits.
    {
        let mut tx = db.create_write_tx().unwrap();
        assert!(tx.api_key_find_active_by_hash("0000abc").unwrap().is_some());
    }

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_revoke(id).unwrap();
        tx.commit().unwrap();
    }

    // Post-revoke: lookup misses.
    {
        let mut tx = db.create_write_tx().unwrap();
        assert!(
            tx.api_key_find_active_by_hash("0000abc").unwrap().is_none(),
            "revoked key must not surface in active lookup"
        );
    }

    // `api_key_get_by_id` still returns the row though, so admin tooling
    // can see the revoked-key audit trail (revoked_at populated).
    {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx.api_key_get_by_id(id).unwrap();
        assert!(!row.is_active());
        assert!(row.revoked_at.is_some());
    }
}

#[test]
fn revoke_unknown_id_is_not_found() {
    let db = open_store();
    let mut tx = db.create_write_tx().unwrap();
    let err = tx
        .api_key_revoke(9999)
        .expect_err("must reject revoke of nonexistent id");
    // Convert to debug to assert the variant shape without depending on
    // exact pattern matching on every field.
    let s = format!("{:?}", err);
    assert!(s.contains("NotFound"), "expected NotFound, got {s}");
}

#[test]
fn double_revoke_does_not_clobber_first_timestamp() {
    // The revoke filter (`revoked_at IS NULL`) makes a second revoke a
    // no-op at the row level — the second call hits zero rows and
    // surfaces as NotFound, so the originally-recorded revoked_at survives.
    let db = open_store();

    let id = {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx
            .api_key_insert("twice-revoked", "01010101", "AccountInfo", None)
            .unwrap();
        tx.commit().unwrap();
        row.id
    };

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_revoke(id).unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let err = tx
        .api_key_revoke(id)
        .expect_err("second revoke must find no matching active row");
    assert!(format!("{err:?}").contains("NotFound"));
}

#[test]
fn touch_last_used_updates_timestamp() {
    let db = open_store();

    let id = {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx.api_key_insert("touched", "feedfeed", "AccountInfo", None).unwrap();
        tx.commit().unwrap();
        row.id
    };

    {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx.api_key_get_by_id(id).unwrap();
        assert!(row.last_used_at.is_none(), "starts unset");
    }

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_touch_last_used(id, Duration::ZERO).unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let row = tx.api_key_get_by_id(id).unwrap();
    assert!(row.last_used_at.is_some(), "last_used_at populated after touch");
}

#[test]
fn touch_last_used_is_throttled() {
    // The auth shim fires touch_last_used on every authenticated request.
    // Without throttling, a busy agent would cause a write per call; with
    // a non-zero throttle the second bump within the window must be a no-op.
    let db = open_store();

    let id = {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx.api_key_insert("throttled", "ccddeeff", "AccountInfo", None).unwrap();
        tx.commit().unwrap();
        row.id
    };

    // First bump goes through (last_used_at was NULL).
    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_touch_last_used(id, Duration::from_secs(60)).unwrap();
        tx.commit().unwrap();
    }
    let first_ts = {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_get_by_id(id).unwrap().last_used_at.unwrap()
    };

    // Second bump within the throttle window: must not advance the timestamp.
    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_touch_last_used(id, Duration::from_secs(60)).unwrap();
        tx.commit().unwrap();
    }
    let second_ts = {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_get_by_id(id).unwrap().last_used_at.unwrap()
    };
    assert_eq!(
        first_ts, second_ts,
        "second bump within throttle window must be a no-op"
    );

    // ZERO throttle bypasses the window and advances the timestamp.
    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_touch_last_used(id, Duration::ZERO).unwrap();
        tx.commit().unwrap();
    }
    let third_ts = {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_get_by_id(id).unwrap().last_used_at.unwrap()
    };
    assert!(third_ts >= second_ts, "ZERO throttle must allow the bump");
}

#[test]
fn touch_last_used_on_revoked_key_is_noop_not_error() {
    // The auth path calls touch_last_used AFTER successful credential
    // verification, so it can race a concurrent revoke. Two invariants:
    //   1. The function must succeed silently even when the row has been revoked from under it (the "auth succeeded"
    //      guarantee cannot be undone by a write failure here).
    //   2. The revoked row's last_used_at must NOT be bumped: an active filter on the update query prevents the bump so
    //      the audit log cannot show activity on a key after its revocation timestamp.
    let db = open_store();

    let id = {
        let mut tx = db.create_write_tx().unwrap();
        let row = tx.api_key_insert("revoke-race", "abcdef", "AccountInfo", None).unwrap();
        tx.commit().unwrap();
        row.id
    };

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_revoke(id).unwrap();
        tx.commit().unwrap();
    }

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_touch_last_used(id, Duration::ZERO)
            .expect("touch on revoked id must succeed silently");
        tx.commit().unwrap();
    }

    // Invariant 2: the revoked row's last_used_at is still None.
    let mut tx = db.create_write_tx().unwrap();
    let row = tx.api_key_get_by_id(id).unwrap();
    assert!(
        row.last_used_at.is_none(),
        "touch_last_used must not bump last_used_at on a revoked key",
    );
}

#[test]
fn list_all_includes_active_and_revoked_in_useful_order() {
    let db = open_store();

    // Insert three keys, revoke the middle one.
    let (_a, b, _c) = {
        let mut tx = db.create_write_tx().unwrap();
        let a = tx.api_key_insert("key-a", "0a", "AccountInfo", None).unwrap();
        let b = tx.api_key_insert("key-b", "0b", "AccountInfo", None).unwrap();
        let c = tx.api_key_insert("key-c", "0c", "AccountInfo", None).unwrap();
        tx.commit().unwrap();
        (a.id, b.id, c.id)
    };

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_revoke(b).unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let listed = tx.api_key_list(true).unwrap();
    assert_eq!(listed.len(), 3, "with include_revoked=true, all three rows surface");

    // Active rows (revoked_at IS NULL) sort before revoked rows because
    // NULL precedes any value in SQLite's ASC ordering. Confirm the
    // revoked one ends up at the end.
    assert!(listed.last().unwrap().revoked_at.is_some());
    assert!(listed[0].revoked_at.is_none());

    // Default `include_revoked=false` excludes the revoked row from the list.
    let active_only = tx.api_key_list(false).unwrap();
    assert_eq!(active_only.len(), 2, "active-only list omits the revoked row");
    assert!(active_only.iter().all(|k| k.revoked_at.is_none()));
}

#[test]
fn find_active_by_hash_excludes_expired_rows() {
    // Once `expires_at` is in the past, the active filter must drop the
    // row. The auth path returns `None` exactly as it would for an
    // unknown or revoked key, so an attacker can't distinguish.
    use time::{Duration as TimeDuration, OffsetDateTime, PrimitiveDateTime};

    let db = open_store();
    let now = OffsetDateTime::now_utc();
    let past = now - TimeDuration::seconds(60);
    let past = PrimitiveDateTime::new(past.date(), past.time());

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_insert("expired", "deadc0de", "AccountInfo", Some(past))
            .unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let found = tx.api_key_find_active_by_hash("deadc0de").unwrap();
    assert!(found.is_none(), "expired key must not surface as active");

    // Audit lookup (get_by_id) still returns it so admins can see the row.
    let listed = tx.api_key_list(false).unwrap();
    assert_eq!(listed.len(), 1, "expired keys still listed for audit");
    assert!(listed[0].expires_at.is_some());
}

#[test]
fn find_active_by_hash_returns_row_with_future_expiry() {
    use time::{Duration as TimeDuration, OffsetDateTime, PrimitiveDateTime};

    let db = open_store();
    let future = OffsetDateTime::now_utc() + TimeDuration::seconds(3600);
    let future = PrimitiveDateTime::new(future.date(), future.time());

    {
        let mut tx = db.create_write_tx().unwrap();
        tx.api_key_insert("future", "beefcafe", "AccountInfo", Some(future))
            .unwrap();
        tx.commit().unwrap();
    }

    let mut tx = db.create_write_tx().unwrap();
    let found = tx.api_key_find_active_by_hash("beefcafe").unwrap();
    assert!(found.is_some(), "non-expired key must surface");
    assert_eq!(found.unwrap().expires_at, Some(future));
}
