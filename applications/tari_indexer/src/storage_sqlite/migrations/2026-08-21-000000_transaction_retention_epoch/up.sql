-- The epoch at which a transaction reached a terminal state and became eligible for retention
-- accounting: its commit epoch once a receipt is indexed, and until then its max_epoch, the last
-- epoch in which it could still be sequenced. Rows written before this column existed carry 0.
alter table transactions
    add column retention_epoch bigint not null default 0;

create index transactions_retention_epoch_idx on transactions (retention_epoch);
