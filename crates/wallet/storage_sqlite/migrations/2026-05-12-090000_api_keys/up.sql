-- API keys issued by an Admin user, used by AI agents / other automated
-- clients to authenticate against the wallet daemon's JSON-RPC API without
-- requiring an interactive webauthn ceremony.
--
-- Security:
--   * Only the SHA-256 hash of the key material is persisted. The raw key
--     is returned to the admin exactly once at creation time and is never
--     recoverable from the database.
--   * Granted permissions are stored as a comma-separated string in the
--     same format as `Permissions`'s `FromStr`/`Display`.
--   * Revocation is soft (`revoked_at`) so we can still log a "last seen"
--     timestamp for already-revoked credentials; queries filter on this.
--   * `expires_at` is a forward-compatibility hook: the auth path will
--     eventually treat any row whose `expires_at` lies in the past as
--     unusable. For the initial implementation it is always written as
--     NULL, but the column exists so adding the enforcement later is a
--     pure code change rather than a follow-up migration.
CREATE TABLE api_keys (
    id              INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    name            TEXT     NOT NULL,
    key_hash        TEXT     NOT NULL,
    permissions     TEXT     NOT NULL,
    created_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at    DATETIME NULL,
    revoked_at      DATETIME NULL,
    expires_at      DATETIME NULL
);

CREATE UNIQUE INDEX api_keys_key_hash_idx ON api_keys (key_hash);
CREATE INDEX        api_keys_name_idx     ON api_keys (name);
