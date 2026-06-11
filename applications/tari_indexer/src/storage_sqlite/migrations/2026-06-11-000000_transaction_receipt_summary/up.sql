alter table transaction_receipts
    add column outcome text not null default 'Commit';
alter table transaction_receipts
    add column total_fees_paid bigint not null default 0;

update transaction_receipts
set outcome         = json_extract(data, '$.outcome'),
    total_fees_paid = json_extract(data, '$.fee_receipt.total_fees_paid');
