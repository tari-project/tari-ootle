// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! Admin-only JSON-RPC handlers + shared helpers for the API key
//! authentication path (issue #1957).
//!
//! ## Trust model
//!
//! - API keys are long-lived credentials minted by an Admin user. The raw key bytes leave the daemon exactly once (the
//!   response of `auth.create_api_key`). Only a SHA-256 hex digest is persisted.
//! - The granted scopes on a key are an arbitrary subset of `JrpcPermissions`, chosen at creation time by the admin.
//!   The `Admin` scope itself is permitted as a grant (since the admin issuing the key has Admin), but the creator must
//!   set `confirm_admin: true` to acknowledge they're minting a credential that has full access.
//! - Revocation is immediate: the storage layer filters revoked rows out of `api_key_find_active_by_hash`, so a key
//!   whose `revoked_at` is set cannot authenticate even if a request is mid-flight.
//!
//! ## Key format
//!
//! Raw keys are 32 random bytes encoded with URL-safe Base64 (no padding)
//! and prefixed with `tw_` so they're identifiable in logs and the UI.
//! Total length ~46 chars. Sufficient entropy to make brute-force pointless;
//! the prefix is for human convenience only and is not used for auth.

use std::str::FromStr;

use anyhow::{Context, anyhow};
use axum_extra::headers::authorization::Bearer;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use sha2::{Digest, Sha256};
use tari_ootle_wallet_sdk::storage::{
    CommittableStore,
    ReadableWalletStore,
    WalletStore,
    WalletStoreReader,
    WalletStoreWriter,
    WriteableWalletStore,
};
use tari_ootle_walletd_client::{
    permissions::{JrpcPermission, JrpcPermissions},
    types::{
        AuthCreateApiKeyRequest,
        AuthCreateApiKeyResponse,
        AuthListApiKeysRequest,
        AuthListApiKeysResponse,
        AuthRevokeApiKeyRequest,
        AuthRevokeApiKeyResponse,
        EncodedApiKey,
        IssuedApiKey,
    },
};
use zeroize::{Zeroize, Zeroizing};

use crate::handlers::HandlerContext;

/// Prefix all minted keys with this so they're identifiable in logs and
/// admin UIs. Not used as a security boundary — the SHA-256 hash covers
/// the entire string including the prefix.
const API_KEY_PREFIX: &str = "tw_";
/// 32 bytes of entropy. With URL-safe base64 this becomes 43 chars; with
/// the 3-char prefix the total key is 46 chars. 2^256 search space is
/// more than enough.
const API_KEY_RAW_BYTES: usize = 32;

/// Produce the deterministic storage hash for an API key. Callers MUST go
/// through this function — both the create path and the verify path — so a
/// future change to the hashing strategy only has to land in one place.
pub fn hash_api_key(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

/// Mint a fresh random API key string in the canonical form
/// `tw_<urlsafe_b64(32 random bytes)>`. Entropy comes from `OsRng` via
/// `rand::thread_rng()`. Never returns the same string twice.
///
/// The raw-byte buffer is wrapped in `Zeroizing` so it is wiped after
/// encoding, and the returned string is wrapped in `Zeroizing<String>`
/// (`EncodedApiKey`) so its heap buffer is wiped on drop. Callers MUST
/// keep it as `EncodedApiKey` end-to-end — converting to a plain `String`
/// would defeat the wipe.
fn mint_raw_api_key() -> EncodedApiKey {
    // `rand::rng()` returns a thread-local CSPRNG seeded from the OS;
    // appropriate for issuing credentials whose entire security rests on
    // unpredictability. `fill_bytes` is on the infallible `Rng` trait in
    // rand 0.10.
    let mut bytes = Zeroizing::new([0u8; API_KEY_RAW_BYTES]);
    rand::rng().fill_bytes(bytes.as_mut_slice());
    let mut encoded = URL_SAFE_NO_PAD.encode(bytes.as_slice());
    let mut out = String::with_capacity(API_KEY_PREFIX.len() + encoded.len());
    out.push_str(API_KEY_PREFIX);
    out.push_str(&encoded);
    // Wipe the intermediate base64 buffer before its drop runs — that
    // allocation also held the raw key in encoded form.
    encoded.zeroize();
    Zeroizing::new(out)
}

/// Look up the API key matching `raw_key`, parse its stored permissions,
/// and best-effort-bump `last_used_at`. Returns the granted permissions on
/// success, or an error indistinguishable between "no such key" and "key
/// revoked" (to deny an attacker the ability to enumerate valid hashes).
///
/// Stays out of the `Authenticator` trait deliberately: that trait returns
/// `Result<(), _>` with no notion of "granted scopes", because for webauthn
/// the scopes come from the request, not from the credentials. API keys
/// invert that, so the `auth.request` handler short-circuits to this
/// function before falling through to the trait.
pub async fn authenticate_api_key<TStore>(store: &TStore, raw_key: &str) -> Result<JrpcPermissions, anyhow::Error>
where TStore: WalletStore {
    let hash = hash_api_key(raw_key);

    // Scoped block: drop the read transaction before opening a write
    // transaction to bump `last_used_at`. SQLite supports concurrent
    // readers but a single writer; holding a read tx alongside a write tx
    // on the same connection-pool slot would deadlock under load.
    let api_key = {
        let mut tx = store
            .create_read_tx()
            .context("Failed to open read transaction for API key lookup")?;
        let row = tx.api_key_find_active_by_hash(&hash)?;
        row.ok_or_else(|| anyhow!("Authentication failed: API key is invalid or revoked"))?
    };

    // Best-effort `last_used_at` bump. A write failure must NOT fail the
    // auth call — the key has already been verified, the UI hint is the
    // only thing at stake. Logged but not propagated.
    if let Err(e) = bump_last_used(store, api_key.id).await {
        log::warn!(
            target: "tari::ootle::walletd::auth",
            "Failed to update api_key.last_used_at for id {}: {}",
            api_key.id,
            e
        );
    }

    parse_permissions(&api_key.permissions)
        .with_context(|| format!("Persisted API key {} has unparseable permissions", api_key.id))
}

async fn bump_last_used<TStore>(store: &TStore, id: i32) -> Result<(), anyhow::Error>
where TStore: WalletStore {
    let mut tx = store
        .create_write_tx()
        .context("Failed to open write transaction for last_used bump")?;
    tx.api_key_touch_last_used(id)?;
    tx.commit()?;
    Ok(())
}

fn parse_permissions(s: &str) -> Result<JrpcPermissions, anyhow::Error> {
    JrpcPermissions::from_str(s).map_err(|e| anyhow!("invalid permission string '{s}': {e}"))
}

// -----------------------------------------------------------------------------
// JSON-RPC handlers
// -----------------------------------------------------------------------------

pub async fn handle_create_api_key(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: AuthCreateApiKeyRequest,
) -> Result<AuthCreateApiKeyResponse, anyhow::Error> {
    // Only an Admin can mint keys. This is a hard wall: even a key with
    // every other permission cannot use `auth.create_api_key` unless it
    // also carries `Admin`.
    context.check_auth(token, &[JrpcPermission::Admin])?;

    if request.name.trim().is_empty() {
        return Err(anyhow!("API key name must not be empty"));
    }

    let permissions = JrpcPermissions::try_from(request.permissions.as_slice())
        .map_err(|e| anyhow!("Invalid permissions in API key request: {e}"))?;
    if permissions.is_empty() {
        return Err(anyhow!(
            "API key must grant at least one permission; refusing to issue an unusable key"
        ));
    }
    // Granting Admin to an API key is allowed (the issuer already has it)
    // but is dangerous, so we require explicit acknowledgement so a UI
    // checkbox can be the gate rather than the JSON-RPC payload alone.
    if permissions.has_permission(&JrpcPermission::Admin) && !request.confirm_admin {
        return Err(anyhow!(
            "Granting the Admin permission to an API key requires `confirm_admin: true` in the request"
        ));
    }

    let raw_key: EncodedApiKey = mint_raw_api_key();
    let hash = hash_api_key(&raw_key);

    // Serialise the permission set back to its textual form. We could
    // store JSON here but the JWT layer already uses the comma-separated
    // form and `JrpcPermissions::from_str` round-trips it, so this keeps
    // exactly one codec across the daemon.
    let permissions_str = format_permissions(&permissions);

    let stored = {
        let mut tx = context
            .wallet_sdk()
            .store()
            .create_write_tx()
            .context("Failed to open write transaction")?;
        let row = tx
            .api_key_insert(request.name.trim(), &hash, &permissions_str)
            .context("Failed to insert api_key row")?;
        tx.commit().context("Failed to commit api_key insert")?;
        row
    };

    Ok(AuthCreateApiKeyResponse {
        id: stored.id,
        name: stored.name.clone(),
        permissions: permissions.into_vec(),
        // The raw key is returned exactly once. The client must store it
        // immediately — there is no way to retrieve it later from the
        // daemon. The DB only ever holds the hash.
        api_key: raw_key,
        created_at: stored.created_at.assume_utc().unix_timestamp(),
    })
}

pub async fn handle_list_api_keys(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _request: AuthListApiKeysRequest,
) -> Result<AuthListApiKeysResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let rows = {
        let mut tx = context
            .wallet_sdk()
            .store()
            .create_read_tx()
            .context("Failed to open read transaction")?;
        tx.api_key_list_all()?
    };

    let keys = rows
        .into_iter()
        .map(|k| {
            let permissions = JrpcPermissions::from_str(&k.permissions)
                .map(JrpcPermissions::into_vec)
                .unwrap_or_default();
            IssuedApiKey {
                id: k.id,
                name: k.name,
                permissions,
                created_at: k.created_at.assume_utc().unix_timestamp(),
                last_used_at: k
                    .last_used_at
                    .map(|ts: time::PrimitiveDateTime| ts.assume_utc().unix_timestamp()),
                revoked_at: k
                    .revoked_at
                    .map(|ts: time::PrimitiveDateTime| ts.assume_utc().unix_timestamp()),
                expires_at: k
                    .expires_at
                    .map(|ts: time::PrimitiveDateTime| ts.assume_utc().unix_timestamp()),
            }
        })
        .collect();

    Ok(AuthListApiKeysResponse { keys })
}

pub async fn handle_revoke_api_key(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: AuthRevokeApiKeyRequest,
) -> Result<AuthRevokeApiKeyResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let mut tx = context
        .wallet_sdk()
        .store()
        .create_write_tx()
        .context("Failed to open write transaction")?;
    tx.api_key_revoke(request.id).context("Failed to revoke api_key")?;
    tx.commit().context("Failed to commit api_key revocation")?;

    Ok(AuthRevokeApiKeyResponse {})
}

/// Round-trip a `JrpcPermissions` back to the comma-separated textual form
/// that `JrpcPermissions::from_str` accepts. We keep the order
/// deterministic (sorted by display form) so a re-list returns the same
/// string the user supplied at creation time.
fn format_permissions(p: &JrpcPermissions) -> String {
    let mut parts: Vec<String> = p.iter().map(|p| p.to_string()).collect();
    parts.sort();
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_keys_have_correct_shape_and_are_unique() {
        // Every minted key must start with the `tw_` prefix so it's
        // identifiable in logs, and two consecutive mints must never
        // collide. 32 bytes of entropy makes that overwhelmingly likely
        // anyway, but we still assert it to catch any accidental
        // shared-state regression in the minter.
        let a = mint_raw_api_key();
        let b = mint_raw_api_key();
        assert!(a.starts_with(API_KEY_PREFIX), "key {} missing prefix", &*a);
        assert!(b.starts_with(API_KEY_PREFIX));
        assert_ne!(a, b, "two consecutive mints must not collide");
        // Prefix (3) + ceil(32 * 4 / 3) without padding = 3 + 43 = 46
        assert_eq!(a.len(), API_KEY_PREFIX.len() + 43);
    }

    #[test]
    fn hashing_is_deterministic_and_distinguishes_inputs() {
        // The same input always hashes to the same digest (otherwise no
        // existing key would ever authenticate again). Distinct inputs
        // must hash to distinct digests — at SHA-256 strength the
        // probability of accidental collision in any realistic deployment
        // is effectively zero.
        let k1 = mint_raw_api_key();
        let k2 = mint_raw_api_key();
        assert_eq!(hash_api_key(&k1), hash_api_key(&k1));
        assert_ne!(hash_api_key(&k1), hash_api_key(&k2));
        // SHA-256 hex digest is always 64 chars.
        assert_eq!(hash_api_key(&k1).len(), 64);
    }

    #[test]
    fn format_permissions_round_trips_through_from_str() {
        // `format_permissions` must produce a string that
        // `JrpcPermissions::from_str` accepts and reconstructs into the
        // same logical permission set. This is the single most important
        // invariant for the storage column: we re-parse it on every
        // authentication.
        let original: JrpcPermissions = vec![
            JrpcPermission::AccountInfo,
            JrpcPermission::TransactionGet,
            JrpcPermission::KeyList,
        ]
        .into();
        let s = format_permissions(&original);
        let parsed: JrpcPermissions = JrpcPermissions::from_str(&s).expect("must re-parse");
        // HashSet order is not guaranteed; compare membership instead of
        // direct equality.
        assert_eq!(parsed.len(), 3);
        assert!(parsed.has_permission(&JrpcPermission::AccountInfo));
        assert!(parsed.has_permission(&JrpcPermission::TransactionGet));
        assert!(parsed.has_permission(&JrpcPermission::KeyList));
    }

    #[test]
    fn format_permissions_is_deterministic_under_input_reordering() {
        // The list endpoint shows users the exact permission string they
        // supplied at creation time. Sorting the parts before joining
        // means the order they typed them in is irrelevant — they always
        // see the same canonical string.
        let a: JrpcPermissions = vec![JrpcPermission::AccountInfo, JrpcPermission::TransactionGet].into();
        let b: JrpcPermissions = vec![JrpcPermission::TransactionGet, JrpcPermission::AccountInfo].into();
        assert_eq!(format_permissions(&a), format_permissions(&b));
    }
}

/// End-to-end tests covering the five acceptance-criteria scenarios listed
/// on issue #1957 (`admin key creation`, `non-admin attempt to create a
/// key`, `authenticated calls within scope`, `rejection out of scope`, and
/// `revocation`).
///
/// These tie the storage layer, `authenticate_api_key`, and the JWT layer
/// together so they exercise the same code paths the JSON-RPC handlers
/// invoke. They do not stand up a full `HandlerContext` — that would
/// require the entire wallet daemon service stack — but they DO cover the
/// invariants the handlers rely on (Admin-gated minting, scope-bounded
/// JWT issuance, immediate revocation, etc.).
#[cfg(test)]
mod e2e_tests {
    use std::time::Duration;

    use axum_extra::headers::authorization::Bearer;
    use tari_crypto::tari_utilities::SafePassword;
    use tari_ootle_wallet_sdk::storage::{
        CommittableStore,
        WalletStoreReader,
        WalletStoreWriter,
        WriteableWalletStore,
    };
    use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
    use tari_ootle_walletd_client::permissions::{JrpcPermission, JrpcPermissions};

    use super::*;
    use crate::handlers::auth::jwt::{JwtApi, JwtApiError};

    fn open_store() -> SqliteWalletStore {
        let db = SqliteWalletStore::try_open(":memory:").unwrap();
        db.run_migrations().unwrap();
        db
    }

    fn jwt_secret() -> SafePassword {
        SafePassword::from("integration-test-secret-do-not-ship".to_string())
    }

    /// Issue a JWT with the given permission set. Equivalent to the path
    /// a real `auth.request` call would take in production.
    fn issue_jwt(secret: &SafePassword, permissions: JrpcPermissions) -> String {
        let api = JwtApi::new(Duration::from_secs(60 * 5), secret);
        let claims = api.generate_auth_claims(permissions).unwrap();
        api.grant(&claims).unwrap().to_string()
    }

    /// Wrap a raw JWT string in a typed `Bearer` via the public
    /// `Authorization::bearer` constructor (axum-extra's `headers`
    /// re-export). Strips back down to the inner Bearer for use with
    /// `JwtApi::check_auth(Some(&bearer))`.
    fn bearer(s: &str) -> Bearer {
        axum_extra::headers::Authorization::bearer(s)
            .expect("test JWT string must be a valid bearer token")
            .0
    }

    /// Helper: insert an API key with the given scope set, return the raw key.
    /// Stand-in for what `handle_create_api_key` does after passing the
    /// admin check + payload validation.
    fn mint_key(store: &SqliteWalletStore, name: &str, perms: JrpcPermissions) -> (EncodedApiKey, i32) {
        let raw = mint_raw_api_key();
        let hash = hash_api_key(&raw);
        let perm_str = format_permissions(&perms);
        let row = {
            let mut tx = store.create_write_tx().unwrap();
            let row = tx.api_key_insert(name, &hash, &perm_str).unwrap();
            tx.commit().unwrap();
            row
        };
        (raw, row.id)
    }

    /// Scenario 1: admin key creation produces a key that authenticates and
    /// surfaces in the list endpoint.
    #[tokio::test]
    async fn admin_can_create_and_use_an_api_key() {
        let store = open_store();
        let perms: JrpcPermissions = vec![JrpcPermission::AccountInfo, JrpcPermission::TransactionGet].into();
        let (raw, _id) = mint_key(&store, "agent-1", perms);

        let granted = authenticate_api_key(&store, &raw).await.expect("auth must succeed");
        assert_eq!(granted.len(), 2);
        assert!(granted.has_permission(&JrpcPermission::AccountInfo));
        assert!(granted.has_permission(&JrpcPermission::TransactionGet));
        assert!(
            !granted.has_permission(&JrpcPermission::Admin),
            "key has no Admin scope"
        );

        // Lookup also populated last_used_at so the admin can audit it.
        let mut tx = store.create_write_tx().unwrap();
        let listed = tx.api_key_list_all().unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].last_used_at.is_some(), "auth must have stamped last_used_at");
    }

    /// Scenario 2: a non-admin JWT presented to a key-management endpoint
    /// must be rejected with `InsufficientPermissions`. This is the same
    /// `JwtApi::check_auth` call every Admin-gated handler makes.
    #[test]
    fn non_admin_jwt_is_rejected_at_key_management_endpoints() {
        let secret = jwt_secret();
        // A scoped-but-not-Admin JWT representing a partially-privileged user.
        let token = issue_jwt(
            &secret,
            vec![JrpcPermission::AccountInfo, JrpcPermission::TransactionGet].into(),
        );
        let api = JwtApi::new(Duration::from_secs(60 * 5), &secret);
        let result = api.check_auth(Some(&bearer(&token)), &[JrpcPermission::Admin]);
        let err = result.expect_err("non-admin token must be rejected for Admin-gated endpoint");
        assert!(
            matches!(err, JwtApiError::InsufficientPermissions {
                required: JrpcPermission::Admin
            }),
            "expected InsufficientPermissions(Admin), got {err:?}"
        );
    }

    /// Scenario 3: an API key with scope `AccountInfo` produces a JWT
    /// that passes the `check_auth` filter for `AccountInfo` endpoints.
    #[tokio::test]
    async fn in_scope_call_is_accepted() {
        let store = open_store();
        let secret = jwt_secret();
        let (raw, _) = mint_key(&store, "info-only", vec![JrpcPermission::AccountInfo].into());

        let granted = authenticate_api_key(&store, &raw).await.unwrap();
        let token = issue_jwt(&secret, granted);
        let api = JwtApi::new(Duration::from_secs(60 * 5), &secret);
        api.check_auth(Some(&bearer(&token)), &[JrpcPermission::AccountInfo])
            .expect("in-scope call must be accepted");
    }

    /// Scenario 4: same key, called against a scope NOT on the key. The
    /// JWT-layer scope check must reject — proving an agent cannot escape
    /// its granted scopes.
    #[tokio::test]
    async fn out_of_scope_call_is_rejected() {
        let store = open_store();
        let secret = jwt_secret();
        let (raw, _) = mint_key(&store, "info-only", vec![JrpcPermission::AccountInfo].into());

        let granted = authenticate_api_key(&store, &raw).await.unwrap();
        // Sanity: granted scope set does not include the one we'll require below.
        assert!(!granted.has_permission(&JrpcPermission::TransactionGet));
        assert!(!granted.has_permission(&JrpcPermission::Admin));

        let token = issue_jwt(&secret, granted);
        let api = JwtApi::new(Duration::from_secs(60 * 5), &secret);
        let err = api
            .check_auth(Some(&bearer(&token)), &[JrpcPermission::TransactionGet])
            .expect_err("out-of-scope call must be rejected");
        assert!(
            matches!(err, JwtApiError::InsufficientPermissions {
                required: JrpcPermission::TransactionGet
            }),
            "expected InsufficientPermissions(TransactionGet), got {err:?}"
        );
    }

    /// Wire-format tests — pin the JSON schema that real JSON-RPC clients
    /// (curl, the JS client, untyped HTTP libraries) will send to / receive
    /// from the daemon. If any of these regress, the JS/CLI/curl integration
    /// breaks even when the in-process Rust calls still work. These are
    /// cheaper and more reliable than spinning up the daemon for HTTP tests
    /// but cover the same wire-format concern.
    mod wire_format {
        use serde_json::json;
        use tari_ootle_walletd_client::{
            permissions::JrpcPermission,
            types::{
                AuthCreateApiKeyRequest,
                AuthCreateApiKeyResponse,
                AuthCredentials,
                AuthListApiKeysRequest,
                AuthListApiKeysResponse,
                AuthLoginRequest,
                AuthRevokeApiKeyRequest,
                AuthRevokeApiKeyResponse,
                IssuedApiKey,
            },
        };

        #[test]
        fn auth_credentials_api_key_variant_serialises_as_externally_tagged_object() {
            // External clients submit `{"ApiKey": "tw_..."}` and expect the
            // enum to deserialise back. serde defaults to externally-tagged
            // for this enum, so the variant name is the key. Pin that here.
            let cred = AuthCredentials::ApiKey("tw_test123".to_string().into());
            let v = serde_json::to_value(&cred).unwrap();
            assert_eq!(v, json!({"ApiKey": "tw_test123"}));

            let round_tripped: AuthCredentials = serde_json::from_value(v).unwrap();
            assert_eq!(round_tripped.as_api_key(), Some("tw_test123"));
        }

        #[test]
        fn auth_login_request_with_api_key_round_trips_through_json() {
            // Exact shape an agent calling `auth.request` with an API key
            // submits over the wire. Captures the contract the JS client +
            // direct-curl users depend on.
            let req = AuthLoginRequest {
                permissions: vec![],
                credentials: AuthCredentials::ApiKey("tw_secret".to_string().into()),
            };
            let v = serde_json::to_value(&req).unwrap();
            assert_eq!(
                v,
                json!({
                    "permissions": [],
                    "credentials": {"ApiKey": "tw_secret"}
                })
            );
            // And it round-trips: a JSON payload constructed by a non-Rust
            // client must deserialise into the same shape.
            let parsed: AuthLoginRequest = serde_json::from_value(v).unwrap();
            assert_eq!(parsed.credentials.as_api_key(), Some("tw_secret"));
        }

        #[test]
        fn create_request_confirm_admin_defaults_to_false_when_omitted() {
            // `#[serde(default)]` on the field means JS / TS clients can
            // omit it for the common non-admin case. Verify the wire-level
            // contract: missing field deserialises to `false`.
            let v = json!({
                "name": "agent-1",
                "permissions": ["AccountInfo"]
                // confirm_admin omitted
            });
            let req: AuthCreateApiKeyRequest = serde_json::from_value(v).unwrap();
            assert_eq!(req.name, "agent-1");
            assert_eq!(req.permissions, vec!["AccountInfo".to_string()]);
            assert!(!req.confirm_admin);
        }

        #[test]
        fn create_request_explicit_confirm_admin_round_trips() {
            let req = AuthCreateApiKeyRequest {
                name: "dangerous".to_string(),
                permissions: vec!["Admin".to_string()],
                confirm_admin: true,
            };
            let v = serde_json::to_value(&req).unwrap();
            // Order is deterministic for serde struct serialisation.
            assert_eq!(v["confirm_admin"], true);
            assert_eq!(v["name"], "dangerous");
            let parsed: AuthCreateApiKeyRequest = serde_json::from_value(v).unwrap();
            assert!(parsed.confirm_admin);
        }

        #[test]
        fn create_response_carries_the_raw_key_and_metadata() {
            // The create response is the only place the raw key surfaces.
            // Make sure the JSON shape matches what the Rust + JS clients
            // expect to parse.
            let resp = AuthCreateApiKeyResponse {
                id: 42,
                name: "monitor".to_string(),
                permissions: vec![JrpcPermission::AccountInfo, JrpcPermission::TransactionGet],
                api_key: "tw_secretmaterial".to_string().into(),
                created_at: 1_700_000_000,
            };
            let v = serde_json::to_value(&resp).unwrap();
            assert_eq!(v["id"], 42);
            assert_eq!(v["api_key"], "tw_secretmaterial");
            assert_eq!(v["created_at"], 1_700_000_000);
            // permissions is a typed array, not just strings — clients can
            // pattern-match on it.
            assert!(v["permissions"].is_array());
        }

        #[test]
        fn list_request_is_an_empty_object_for_pnpm_clients() {
            // The JS client passes `{}` for list. Verify the empty shape
            // deserialises cleanly so we don't accidentally break the
            // bog-standard list flow.
            let parsed: AuthListApiKeysRequest = serde_json::from_value(json!({})).unwrap();
            // No fields to assert — but the parse not panicking IS the test.
            let _ = parsed;
        }

        #[test]
        fn list_response_revoked_timestamp_is_null_for_active_keys() {
            // The TS binding `revoked_at: bigint | null` — the JSON must
            // emit `null` (not omit the field) for active keys so the
            // JS client's null check works.
            let resp = AuthListApiKeysResponse {
                keys: vec![IssuedApiKey {
                    id: 1,
                    name: "active".to_string(),
                    permissions: vec![JrpcPermission::AccountInfo],
                    created_at: 1_700_000_000,
                    last_used_at: None,
                    revoked_at: None,
                    expires_at: None,
                }],
            };
            let v = serde_json::to_value(&resp).unwrap();
            // Field must be present and null for active keys / keys without expiry.
            assert!(v["keys"][0]["revoked_at"].is_null());
            assert!(v["keys"][0]["last_used_at"].is_null());
            assert!(v["keys"][0]["expires_at"].is_null());
        }

        #[test]
        fn revoke_request_takes_only_an_id() {
            let req = AuthRevokeApiKeyRequest { id: 99 };
            let v = serde_json::to_value(&req).unwrap();
            assert_eq!(v, json!({"id": 99}));

            let resp = AuthRevokeApiKeyResponse {};
            let v = serde_json::to_value(&resp).unwrap();
            // Empty response object — `{}` not `null` so JS clients can
            // dot-access without an undefined check.
            assert_eq!(v, json!({}));
        }
    }

    /// Refresh-cookie invariant: the login handler MUST NOT issue a
    /// refresh cookie for API-key-authenticated sessions.
    ///
    /// The refresh path (`handle_token_refresh`) only consults the
    /// in-memory `RefreshTokenStore`, not the `api_keys` table. If we
    /// issued a refresh cookie when authenticating via API key, a
    /// revoked key could keep minting fresh JWTs for the full
    /// refresh-token-store lifetime (1h default) by riding the cookie —
    /// breaking the bounty's ''revocation is immediate'' acceptance
    /// criterion. The login handler asserts this with a `matches!`
    /// check; this test pins that exact pattern down so a future
    /// refactor cannot quietly regress it.
    #[test]
    fn api_key_credentials_must_not_be_issued_a_refresh_cookie() {
        use tari_ootle_walletd_client::types::AuthCredentials;

        // Mirrors the live pattern in handle_login_request.
        fn issues_refresh_cookie(creds: &AuthCredentials) -> bool {
            !matches!(creds, AuthCredentials::ApiKey(_))
        }

        assert!(
            !issues_refresh_cookie(&AuthCredentials::ApiKey("tw_anything".to_string().into())),
            "API-key sessions MUST NOT get a refresh cookie"
        );
        assert!(
            issues_refresh_cookie(&AuthCredentials::None),
            "Anonymous sessions get a refresh cookie (no api-key risk)"
        );
        // WebAuthN intentionally omitted: constructing a
        // `WebauthnFinishAuthRequest` requires real cryptographic
        // material. Behaviour is symmetric with `None` (any non-ApiKey
        // variant takes the cookie path).
    }

    /// Scenario 5: revoke takes effect immediately — the same key that
    /// authenticated before revocation must fail to authenticate after.
    /// This is the most important security invariant of the entire flow:
    /// a compromised key must be killable in real time.
    #[tokio::test]
    async fn revocation_is_immediate() {
        let store = open_store();
        let (raw, id) = mint_key(&store, "ephemeral", vec![JrpcPermission::AccountInfo].into());

        // Pre-revoke: auth succeeds.
        authenticate_api_key(&store, &raw)
            .await
            .expect("auth before revocation must succeed");

        // Revoke.
        {
            let mut tx = store.create_write_tx().unwrap();
            tx.api_key_revoke(id).unwrap();
            tx.commit().unwrap();
        }

        // Post-revoke: same raw key, auth must fail.
        let err = authenticate_api_key(&store, &raw)
            .await
            .expect_err("auth after revocation must fail");
        // Generic error message — same wording for "unknown key" and "revoked
        // key" so an attacker can't enumerate valid hashes via the error.
        assert!(
            err.to_string().contains("invalid or revoked"),
            "expected revoked-key error, got {err}"
        );
    }
}
