DROP INDEX locks_transaction_id;
CREATE UNIQUE INDEX locks_uniq_transaction_id ON locks (transaction_id) WHERE transaction_id IS NOT NULL;
