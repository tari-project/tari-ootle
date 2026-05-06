CREATE TABLE wallet_api_keys
(
    id           TEXT      NOT NULL PRIMARY KEY,
    name         TEXT      NOT NULL,
    key_hash     BLOB      NOT NULL,
    permissions  BLOB      NOT NULL,
    last_used_at TIMESTAMP NULL,
    revoked_at   TIMESTAMP NULL,
    created_at   TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX wallet_api_keys_key_hash_idx ON wallet_api_keys (key_hash);
CREATE INDEX wallet_api_keys_active_idx ON wallet_api_keys (revoked_at);
