CREATE TABLE api_keys (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    key_hash    TEXT NOT NULL UNIQUE,
    scopes      TEXT NOT NULL,
    created_at  BIGINT NOT NULL,
    expires_at  BIGINT,
    last_used   BIGINT,
    revoked     INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_api_keys_hash ON api_keys(key_hash);
