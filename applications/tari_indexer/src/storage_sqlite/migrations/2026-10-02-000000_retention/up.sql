drop table substate_transitions;

-- The epoch the transaction committed in. Retention ages receipts and their events out by it.
alter table transaction_receipts add column epoch bigint not null default 0;
update transaction_receipts set epoch = coalesce(json_extract(data, '$.epoch'), 0);
create index transaction_receipts_epoch_idx on transaction_receipts (epoch);

alter table events add column epoch bigint not null default 0;
update events
set epoch = coalesce((select r.epoch from transaction_receipts r where r.address = events.tx_hash), 0);
create index events_epoch_idx on events (epoch);

-- Receipts are counted as they are indexed, so the total covers receipts that retention has since pruned.
insert into key_values (key, value)
select 'transaction_receipt_count', cast(count(*) as text)
from transaction_receipts;
