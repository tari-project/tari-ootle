delete from key_values where key = 'transaction_receipt_count';

drop index events_epoch_idx;
alter table events drop column epoch;

drop index transaction_receipts_epoch_idx;
alter table transaction_receipts drop column epoch;

create table substate_transitions
(
    id            integer   not NULL primary key AUTOINCREMENT,
    shard         int       not NULL,
    state_version bigint    not NULL,
    epoch         bigint    not NULL,
    substate_id   text      not NULL,
    version       bigint    not NULL,
    substate_type text      not NULL,
    is_up         bool      not NULL,
    value_hash    text      NULL,
    created_at    timestamp not null default current_timestamp
);

create unique index substate_transitions_substate_id_version_uniq on substate_transitions (substate_id, version, is_up);
create index substate_transitions_shard_state_version_idx on substate_transitions (shard, state_version);
