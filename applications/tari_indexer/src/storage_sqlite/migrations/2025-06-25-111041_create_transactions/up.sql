create table transactions
(
    id             integer   not NULL primary key AUTOINCREMENT,
    transaction_id text      not NULL,
    body           text      not null,
    created_at     timestamp not null default current_timestamp
);

create unique index transactions_transaction_id_uniq_idx on transactions (transaction_id);