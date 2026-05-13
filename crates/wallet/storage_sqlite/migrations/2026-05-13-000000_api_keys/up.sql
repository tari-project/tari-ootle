CREATE TABLE api_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    permissions TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP NOT NULL,
    last_used_at DATETIME NULL,
    expires_at DATETIME NULL,
    revoked_at DATETIME NULL
);

CREATE INDEX api_keys_revoked_at_idx ON api_keys (revoked_at);
CREATE INDEX api_keys_expires_at_idx ON api_keys (expires_at);
CREATE INDEX api_keys_name_idx ON api_keys (name);
