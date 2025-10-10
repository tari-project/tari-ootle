PRAGMA foreign_keys = ON;

-- Key Manager
CREATE TABLE key_manager_states
(
    id          INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    branch_seed TEXT                              NOT NULL,
    `index`     BIGINT                            NOT NULL,
    is_active   BOOLEAN                           NOT NULL,
    created_at  DATETIME                          NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME                          NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX key_manager_states_uniq_branch_seed_index on key_manager_states (branch_seed, `index`);

CREATE TABLE key_manager_imported_keys
(
    id               INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    label            TEXT                              NOT NULL,
    encrypted_secret BLOB                              NOT NULL,
    key_type         TEXT                              NOT NULL,
    created_at       DATETIME                          NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX key_manager_imported_keys_uniq_label on key_manager_imported_keys (label);


-- Config

CREATE TABLE config
(
    id           INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    key          TEXT                              NOT NULL,
    value        TEXT                              NOT NULL,
    is_encrypted BOOLEAN                           NOT NULL,
    created_at   DATETIME                          NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   DATETIME                          NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX config_uniq_key on config (key);

-- Transaction
CREATE TABLE transactions
(
    id                    INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    transaction_id        TEXT     NOT NULL,
    transaction_json      TEXT     NOT NULL,
    referenced_components TEXT     NOT NULL,
    signers               TEXT     NOT NULL,
    result                TEXT     NULL,
    qcs                   TEXT     NULL,
    final_fee             BIGINT   NULL,
    status                TEXT     NOT NULL,
    dry_run               BOOLEAN  NOT NULL,
    executed_time_ms      BIGINT   NULL,
    finalized_time        DATETIME NULL,
    new_account_info      TEXT     NULL,
    invalid_reason        TEXT     NULL,
    created_at            DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at            DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX transactions_transaction_id_uniq ON transactions (transaction_id);
CREATE INDEX transactions_idx_status ON transactions (status);

-- Substates
CREATE TABLE substates
(
    id                   INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    module_name          TEXT     NULL,
    address              TEXT     NOT NULL,
    parent_address       TEXT     NULL,
    referenced_substates TEXT     NOT NULL,
    version              INTEGER  NOT NULL,
    template_address     TEXT     NULL,
    created_at           DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX substates_uniq_address ON substates (address);

-- Accounts
CREATE TABLE accounts
(
    id                    INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    name                  TEXT     NULL,
    address               TEXT     NOT NULL,
    owner_public_key      TEXT     NOT NULL,
    view_only_key_id      TEXT     NOT NULL,
    owner_key_id          TEXT     NULL,
    is_default            BOOLEAN  NOT NULL DEFAULT 0,
    is_confirmed_on_chain BOOLEAN  NOT NULL,
    stealth_resources     TEXT     NOT NULL DEFAULT '[]',
    created_at            DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at            DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX accounts_uniq_address ON accounts (address);
CREATE UNIQUE INDEX accounts_uniq_owner_public_key ON accounts (owner_public_key);
CREATE UNIQUE INDEX accounts_uniq_name ON accounts (name) WHERE name IS NOT NULL;

-- Vaults
CREATE TABLE vaults
(
    id                      INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    account_id              INTEGER  NOT NULL REFERENCES accounts (id),
    address                 TEXT     NOT NULL,
    resource_address        TEXT     NOT NULL,
    resource_type           TEXT     NOT NULL,
    revealed_balance        BIGINT   NOT NULL DEFAULT 0,
    confidential_balance    BIGINT   NOT NULL DEFAULT 0,
    locked_revealed_balance BIGINT   NOT NULL DEFAULT 0,
    token_symbol            TEXT     NULL,
    divisibility            INTEGER  NOT NULL DEFAULT 0,
    locked_by               INTEGER  NULL REFERENCES locks (id) ON DELETE SET NULL,
    created_at              DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at              DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX vaults_uniq_address ON vaults (address);

-- Resources
CREATE TABLE resources
(
    id            INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    address       TEXT     NOT NULL,
    resource_type TEXT     NOT NULL,
    owner_key     TEXT     NULL,
    owner_rule    TEXT     NOT NULL,
    access_rules  TEXT     NOT NULL,
    token_symbol  TEXT     NULL,
    divisibility  INTEGER  NOT NULL,
    metadata      TEXT     NOT NULL,
    total_supply  TEXT     NULL,
    view_key      TEXT     NULL,
    auth_hook     TEXT     NULL,

    updated_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX resources_uniq_address ON resources (address);

-- Confidential Outputs
CREATE TABLE confidential_outputs
(
    id                  INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    account_id          INTEGER  NOT NULL REFERENCES accounts (id),
    vault_id            INTEGER  NOT NULL REFERENCES vaults (id),
    commitment          TEXT     NOT NULL,
    value               BIGINT   NOT NULL,
    sender_public_nonce TEXT     NULL,
    view_only_key_id    TEXT     NOT NULL,
    owner_key_id        TEXT     NULL,
    public_asset_tag    TEXT     NULL,
    memo_json           TEXT     NULL,
    -- Status can be "Unspent", "Spent", "Locked", "LockedUnconfirmed", "Invalid"
    status              TEXT     NOT NULL,
    locked_at           DATETIME NULL,
    lock_id             INTEGER  NULL,
    encrypted_data      blob     NOT NULL DEFAULT '',
    created_at          DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at          DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX confidential_outputs_uniq_commitment ON confidential_outputs (commitment);
CREATE INDEX confidential_outputs_idx_account_status ON confidential_outputs (account_id, status);

-- Locks
CREATE TABLE locks
(
    id             INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    transaction_id TEXT     NULL,
    created_at     DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Auth token, we don't store the auth token, the token in this table is the jwt token that is granted when user accepts the auth login request.
CREATE TABLE auth_status
(
    id           INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    user_decided BOOLEAN                           NOT NULL,
    granted      BOOLEAN                           NOT NULL,
    token        TEXT                              NULL,
    revoked      BOOLEAN                           NOT NULL DEFAULT FALSE
);

-- NFTs
CREATE TABLE non_fungible_tokens
(
    id           INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    vault_id     INTEGER  NOT NULL REFERENCES vaults (id),
    nft_id       TEXT     NOT NULL,
    resource_id  text     NOT NULL,
    data         TEXT     NOT NULL,
    mutable_data TEXT     NOT NULL,
    is_burnt     BOOLEAN  NOT NULL,
    created_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX nfts_uniq_address ON non_fungible_tokens (nft_id);
CREATE UNIQUE INDEX nfts_uniq_address_vault_id_uniq_idx ON non_fungible_tokens (nft_id, vault_id);

CREATE TABLE authored_templates
(
    id                INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    author_public_key TEXT     NOT NULL,
    address           TEXT     NOT NULL,
    name              TEXT     NOT NULL,
    tari_version      TEXT     NOT NULL,
    functions         TEXT     NOT NULL,
    created_at        DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at        DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE webauthn_registrations
(
    id         INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    username   TEXT     NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX webauthn_regs_usernames ON webauthn_registrations (username);

CREATE TABLE webauthn_registration_passkeys
(
    id              INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    registration_id INTEGER  NOT NULL REFERENCES webauthn_registrations (id),
    passkey         BLOB     NOT NULL,
    created_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Stealth Outputs
CREATE TABLE stealth_outputs
(
    id                  INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    owner_account_id    INTEGER  NOT NULL REFERENCES accounts (id),
    resource_address    TEXT     NOT NULL,
    commitment          TEXT     NOT NULL,
    value               TEXT     NOT NULL,
    sender_public_nonce TEXT     NOT NULL,
    -- Status can be "Unspent", "Spent", "Locked", "LockedUnconfirmed", "Invalid"
    status              TEXT     NOT NULL,
    locked_at           DATETIME NULL,
    lock_id             INTEGER  NULL,
    view_only_key_id    TEXT     NOT NULL,
    owner_key_id        TEXT     NULL,
    encrypted_data      BLOB     NOT NULL DEFAULT '',
    tag_byte            INTEGER  NOT NULL,
    memo_json           TEXT     NULL,
    is_burnt            BOOLEAN  NOT NULL DEFAULT 0,
    is_frozen           BOOLEAN  NOT NULL DEFAULT 0,
    is_on_chain         BOOLEAN  NOT NULL,
    created_at          DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at          DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX stealth_outputs_uniq_resource_addr_commitment ON stealth_outputs (resource_address, commitment);
CREATE INDEX stealth_outputs_idx_resource_status ON stealth_outputs (resource_address, status);

-- Shard State Versions
CREATE TABLE shard_state_versions
(
    id            INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    account_id    INTEGER  NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    resource_id   INTEGER  NOT NULL REFERENCES resources (id) ON DELETE CASCADE,
    shard         INTEGER  NOT NULL,
    state_version BIGINT   NOT NULL,
    created_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX shard_state_versions_account_resource_shard_uniq ON shard_state_versions (account_id, resource_id, shard);
CREATE INDEX shard_state_versions_account_resource_shard_state_version_idx ON shard_state_versions (account_id, resource_id, shard, state_version);

-- UTXO process queue
CREATE TABLE utxo_process_queue
(
    id               INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    account_id       INTEGER  NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    resource_address TEXT     NOT NULL,
    utxo_tag         INT      NOT NULL,
    public_nonce     TEXT     NOT NULL,
    created_at       DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX utxo_process_queue_account_resource_tag_nonce_uniq
    ON utxo_process_queue (account_id, resource_address, utxo_tag, public_nonce);