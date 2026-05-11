//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Diesel models for the `api_keys` table.
//!
//! API keys are long-lived credentials issued by an Admin user so AI agents
//! (and other automated clients) can authenticate to the wallet daemon's
//! JSON-RPC API without interactive webauthn. The raw key bytes are
//! returned to the admin exactly once at creation; only a SHA-256 hash is
//! ever persisted, so a database leak cannot grant access.

use diesel::{AsChangeset, Identifiable, Insertable, Queryable};
use time::PrimitiveDateTime;

use crate::schema::api_keys;

/// One row of the `api_keys` table.
///
/// `permissions` is the textual `JrpcPermissions` form (comma-separated;
/// see `JrpcPermissions::from_str`) so the schema does not have to track
/// individual permission variants.
#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = api_keys)]
pub struct ApiKey {
    pub id: i32,
    pub name: String,
    pub key_hash: String,
    pub permissions: String,
    pub created_at: PrimitiveDateTime,
    pub last_used_at: Option<PrimitiveDateTime>,
    pub revoked_at: Option<PrimitiveDateTime>,
}

impl ApiKey {
    /// Is this key currently usable (not revoked)?
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }
}

/// Insert shape for [`ApiKey`]. `key_hash` is the SHA-256 hex digest of the
/// raw key bytes; the raw key is never persisted in any form.
#[derive(Debug, Insertable)]
#[diesel(table_name = api_keys)]
pub struct NewApiKey<'a> {
    pub name: &'a str,
    pub key_hash: &'a str,
    pub permissions: &'a str,
}

/// `last_used_at` bump performed on every successful authentication using a
/// key. Used by the UI to surface stale credentials a user might want to
/// revoke. We do not update `last_used_at` via `dsl::now` so the caller can
/// pass a consistent timestamp across batched updates if needed.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = api_keys)]
pub struct ApiKeyLastUsedChangeset {
    pub last_used_at: Option<PrimitiveDateTime>,
}

/// Revocation marker. Soft-delete: the row is retained so the historical
/// `last_used_at` is available even after revoke.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = api_keys)]
pub struct ApiKeyRevocationChangeset {
    pub revoked_at: Option<PrimitiveDateTime>,
}
