drop index transactions_retention_epoch_idx;
alter table transactions
    drop column retention_epoch;
