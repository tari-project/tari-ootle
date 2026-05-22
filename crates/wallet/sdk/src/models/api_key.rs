//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! SDK-level representation of an issued API key.
//!
//! The raw key material is intentionally NOT modelled here: it exists only
//! for the duration of a single `auth.create_api_key` JSON-RPC call and is
//! returned to the caller exactly once. The DB row, and therefore this
//! struct, only ever holds the SHA-256 hash. A leak of either the database
//! or this struct in flight cannot grant access.

use time::PrimitiveDateTime;

// NOTE: `ApiKey` is the internal SDK-layer model returned by the storage
// reader. It is not a JSON-RPC wire type — the wire type lives in
// `tari_ootle_walletd_client::types::IssuedApiKey` and uses `i64` unix
// timestamps (which ts-rs can export) rather than `PrimitiveDateTime`. So
// we deliberately do NOT derive `ts_rs::TS` on this struct.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiKey {
    pub id: i32,
    /// Human-readable label the admin chose at creation. Not unique.
    pub name: String,
    /// SHA-256 hex digest of the raw key bytes. Index for fast lookup.
    pub key_hash: String,
    /// Textual `Permissions` form (comma-separated). The auth layer
    /// parses this on every credential presentation; we keep the same
    /// format already used by the JWT claims so there is exactly one
    /// permission string codec in the wallet daemon.
    pub permissions: String,
    pub created_at: PrimitiveDateTime,
    /// Bumped on every successful authentication using this key. Lets the
    /// admin spot stale credentials to revoke.
    pub last_used_at: Option<PrimitiveDateTime>,
    /// Soft-delete: rows are retained for audit / historical "last_used"
    /// surfacing, but a non-null `revoked_at` makes the key unusable.
    pub revoked_at: Option<PrimitiveDateTime>,
    /// Optional expiry timestamp. Always written as `None` for now; the
    /// auth path's active-row filter does NOT yet enforce it. The field
    /// exists so the enforcement can be added later without a follow-up
    /// schema migration.
    pub expires_at: Option<PrimitiveDateTime>,
}

impl ApiKey {
    /// Convenience: the key is usable iff it has not been revoked.
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }
}
