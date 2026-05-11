-- API keys issued by an Admin user, used by AI agents / other automated
-- clients to authenticate against the wallet daemon's JSON-RPC API without
-- requiring an interactive webauthn ceremony.
--
-- Security:
--   * Only the SHA-256 hash of the key material is persisted. The raw key
--     is returned to the admin exactly once at creation time and is never
--     recoverable from the database.
--   * Granted permissions are stored as a comma-separated string in the
--     same format as `JrpcPermissions`'s `FromStr`/`Display`.
--   * Revocation is soft (`revoked_at`) so we can still log a "last seen"
--     timestamp for already-revoked credentials; queries filter on this.
CREATE TABLE api_keys (
    id              INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    name            TEXT     NOT NULL,
    key_hash        TEXT     NOT NULL,
    permissions     TEXT     NOT NULL,
    created_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at    DATETIME NULL,
    revoked_at      DATETIME NULL
);

CREATE UNIQUE INDEX api_keys_key_hash_idx ON api_keys (key_hash);
CREATE INDEX        api_keys_name_idx     ON api_keys (name);
