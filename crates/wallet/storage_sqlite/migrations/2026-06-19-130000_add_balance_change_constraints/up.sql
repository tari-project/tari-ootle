-- Add partial unique index to prevent duplicate transaction entries
-- for the same vault. Only applies when transaction_id is not null,
-- so Scan/Recovery entries are not affected.
CREATE UNIQUE INDEX IF NOT EXISTS uq_account_balance_changes_vault_tx
    ON account_balance_changes (vault_id, transaction_id)
    WHERE transaction_id IS NOT NULL;
