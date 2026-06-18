CREATE TABLE account_balance_changes (
    id                          INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    vault_id                    INTEGER NOT NULL REFERENCES vaults (id) ON DELETE CASCADE,
    account_id                  INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    resource_address            TEXT    NOT NULL,
    before_revealed_balance     TEXT    NOT NULL,
    after_revealed_balance      TEXT    NOT NULL,
    before_confidential_balance TEXT    NOT NULL,
    after_confidential_balance  TEXT    NOT NULL,
    revealed_delta              TEXT    NOT NULL,
    confidential_delta          TEXT    NOT NULL,
    source                      TEXT    NOT NULL,
    transaction_id              TEXT,
    created_at                  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX account_balance_changes_account_created_at_idx ON account_balance_changes (account_id, created_at DESC);
CREATE INDEX account_balance_changes_vault_idx ON account_balance_changes (vault_id);
CREATE INDEX account_balance_changes_tx_idx ON account_balance_changes (transaction_id);
