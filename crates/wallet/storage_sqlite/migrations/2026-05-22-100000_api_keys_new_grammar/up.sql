-- The Permission redesign changes the wire/storage grammar from the old
-- PascalCase variants (`AccountInfo`, `TransactionSend_<id>`, ...) to a
-- lowercase `<resource>:<action>[:<entity>]` form (`accounts:read`,
-- `transfer:create:component_…`, ...). Existing rows would fail to
-- re-parse on the next authentication, so we drop them. Ootle is
-- pre-launch — admins re-mint with `wallet auth api-key create` using
-- the new grammar.
--
-- Schema is identical to the prior migration; only contents reset.
DROP INDEX IF EXISTS api_keys_name_idx;
DROP INDEX IF EXISTS api_keys_key_hash_idx;
DROP TABLE IF EXISTS api_keys;

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
