alter table transactions
    add column rejected_reason text null;
alter table transactions
    add column rejected_at timestamp null;
