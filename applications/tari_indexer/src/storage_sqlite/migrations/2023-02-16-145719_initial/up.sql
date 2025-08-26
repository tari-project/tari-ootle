create table substates
(
    id               integer   not NULL primary key AUTOINCREMENT,
    address          text      not NULL,
    version          int       not NULL,
    data             text      not NULL,
    template_address text      NULL,
    module_name      text      NULL,
    -- Block timestamp
    timestamp        timestamp not NULL,
    updated_at       timestamp not null default current_timestamp,
    created_at       timestamp not null default current_timestamp
);

create unique index uniq_substates_address on substates (address);

create table substate_transitions
(
    id            integer   not NULL primary key AUTOINCREMENT,
    shard         int       not NULL,
    state_version bigint    not NULL,
    epoch         bigint    not NULL,
    substate_id   text      not NULL,
    version       int       not NULL,
    substate_type text      not NULL,
    is_up         bool      not NULL,
    value_hash    text      NULL,
    created_at    timestamp not null default current_timestamp
);

create unique index substate_transitions_substate_id_version_uniq on substate_transitions (substate_id, version, is_up);
create index substate_transitions_shard_state_version_idx on substate_transitions (shard, state_version);

-- all the indexes in NFT resources
create table non_fungible_indexes
(
    id                   integer not NULL primary key AUTOINCREMENT,
    resource_address     text    not NULL,
    idx                  integer not NULL,
    non_fungible_address text    not NULL,
    FOREIGN KEY (resource_address) REFERENCES substates (address),
    FOREIGN KEY (non_fungible_address) REFERENCES substates (address)
);

-- A list can only have one single item at any specific position
create unique index uniq_nft_indexes on non_fungible_indexes (resource_address, idx);

-- DB index for faster collection scan queries
create index nft_indexes_resource on non_fungible_indexes (resource_address, idx);

-- Event data
create table events
(
    id               integer   not NULL primary key AUTOINCREMENT,
    template_address text      not NULL,
    tx_hash          text      not NULL,
    topic            text      not NULL,
    payload          text      not NULL,
    substate_id      text      NULL,
    created_at       timestamp not null default current_timestamp
);


-- DB index for faster collection scan queries
create index events_indexer on events (template_address, tx_hash);

-- Latest scanned blocks, separately by committee (epoch + shard)
-- Used mostly for efficient scanning of events in the whole network
create table scanned_block_ids
(
    id            integer not NULL primary key AUTOINCREMENT,
    epoch         bigint  not NULL,
    shard_group   integer not null,
    last_block_id blob    not null
);


-- There should only be one last scanned block by committee (epoch + shard)
create unique index scanned_block_ids_unique_committee on scanned_block_ids (epoch, shard_group);

-- DB index for faster retrieval of the latest block by committee
create index scanned_block_ids_committee on scanned_block_ids (epoch, shard_group);

create table transactions
(
    id             integer   not NULL primary key AUTOINCREMENT,
    transaction_id text      not NULL,
    body           text      not null,
    created_at     timestamp not null default current_timestamp
);

create unique index transactions_transaction_id_uniq_idx on transactions (transaction_id);

-- General purpose key value table
CREATE TABLE key_values
(
    id         INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    key        TEXT                              NOT NULL,
    value      TEXT                              NOT NULL,
    created_at DATETIME                          NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME                          NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX key_values_uniq_key on key_values (key);

-- Epoch checkpoints
CREATE TABLE epoch_checkpoints
(
    id          INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    epoch       BIGINT                            NOT NULL,
    shard_group TEXT                              NOT NULL,
    json_data   TEXT                              NOT NULL,
    created_at  DATETIME                          NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME                          NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX epoch_checkpoints_uniq_epoch_shard_group ON epoch_checkpoints (epoch, shard_group);

create table utxos
(
    id               integer   not NULL primary key AUTOINCREMENT,
    address          text      not NULL,
    version          int       not NULL,
    resource_address text      not NULL,
    shard            int       not NULL,
    state_version    bigint    not NULL,
    output           text      NULL,
    utxo_tag_byte    int       NULL,
    is_spent         boolean   not NULL,
    is_burnt         boolean   not NULL,
    is_frozen        boolean   not NULL,
    created_at       timestamp not null default current_timestamp
);

CREATE INDEX utxos_shard_tag_resource_state_version_idx
    ON utxos (shard, utxo_tag_byte, resource_address, state_version);

