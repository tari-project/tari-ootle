-- Committee-validated state merkle roots, recorded by the network state sync worker and consulted by
-- the read path to skip per-read commit-proof (QC chain) re-validation when a served root is already
-- trusted. The trust key is (epoch, shard_group, state_merkle_root); block_hash is diagnostics-only.
-- A small bounded ring of recent roots per (epoch, shard_group) is retained so reads landing on a
-- validator slightly behind the indexer's last probe still hit a trusted root.
create table verified_state_roots
(
    id                integer   not null primary key autoincrement,
    epoch             bigint    not null,
    shard_group       text      not null,
    block_height      bigint    not null,
    block_hash        text      not null,
    state_merkle_root text      not null,
    validated_at      timestamp not null default current_timestamp,
    unique (epoch, shard_group, state_merkle_root)
);

create index idx_verified_state_roots_lookup on verified_state_roots (epoch, shard_group, state_merkle_root);
create index idx_verified_state_roots_latest on verified_state_roots (epoch, shard_group, block_height);
