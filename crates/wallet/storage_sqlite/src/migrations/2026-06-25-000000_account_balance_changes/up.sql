-- account_balance_changes: persistent log of vault balance mutations
-- Additive migration - existing wallets upgrade without data loss
CREATE TABLE IF NOT EXISTS account_balance_changes (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    vault_id                BLOB    NOT NULL,
    account_address         BLOB    NOT NULL,
    resource_address        BLOB    NOT NULL,

    -- Balances before the change (microtari)
    revealed_balance_before INTEGER NOT NULL DEFAULT 0,
    confidential_balance_before INTEGER NOT NULL DEFAULT 0,

    -- Balances after the change (microtari)
    revealed_balance_after  INTEGER NOT NULL DEFAULT 0,
    confidential_balance_after INTEGER NOT NULL DEFAULT 0,

    -- Signed deltas (after - before)
    revealed_delta          INTEGER NOT NULL DEFAULT 0,
    confidential_delta      INTEGER NOT NULL DEFAULT 0,

    -- Source: 0 = Transaction, 1 = Scan, 2 = Recovery
    source                  INTEGER NOT NULL DEFAULT 1,

    -- Nullable: present for Transaction source, NULL for Scan/Recovery
    transaction_id          BLOB,

    created_at              DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Idempotency: prevent duplicate entries for same vault+tx
    UNIQUE(vault_id, transaction_id, source)
        ON CONFLICT IGNORE
);

CREATE INDEX IF NOT EXISTS idx_balance_changes_account
    ON account_balance_changes(account_address, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_balance_changes_vault
    ON account_balance_changes(vault_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_balance_changes_tx
    ON account_balance_changes(transaction_id)
    WHERE transaction_id IS NOT NULL;
