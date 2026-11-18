-- A transaction may hold several locks, e.g. one per stealth transfer statement it spends.
DROP INDEX locks_uniq_transaction_id;
CREATE INDEX locks_transaction_id ON locks (transaction_id) WHERE transaction_id IS NOT NULL;
