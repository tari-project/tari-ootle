CREATE TABLE api_keys (
    id         INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    name       TEXT     NOT NULL,
    key_hash   BLOB     NOT NULL UNIQUE,
    permissions TEXT    NOT NULL,
    created_at BIGINT   NOT NULL,
    last_used_at BIGINT,
    revoked_at BIGINT
);
