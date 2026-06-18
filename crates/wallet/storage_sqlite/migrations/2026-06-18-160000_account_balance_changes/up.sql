CREATE TABLE account_balance_changes
(
    id                  INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    account_id          INTEGER  NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    vault_id            INTEGER  NOT NULL REFERENCES vaults (id) ON DELETE CASCADE,
    resource_address    TEXT     NOT NULL,
    source_type         TEXT     NOT NULL CHECK (source_type IN ('transaction', 'scan', 'recovery')),
    transaction_id      TEXT     NULL REFERENCES transactions (transaction_id),
    revealed_before     TEXT     NOT NULL,
    revealed_after      TEXT     NOT NULL,
    confidential_before TEXT     NOT NULL,
    confidential_after  TEXT     NOT NULL,
    revealed_delta      TEXT     NOT NULL,
    confidential_delta  TEXT     NOT NULL,
    created_at          DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (
        (source_type = 'transaction' AND transaction_id IS NOT NULL) OR
        (source_type IN ('scan', 'recovery') AND transaction_id IS NULL)
    ),
    CHECK (revealed_before <> revealed_after OR confidential_before <> confidential_after),
    CHECK (revealed_delta <> '0' OR confidential_delta <> '0')
);

CREATE INDEX account_balance_changes_account_created_idx
    ON account_balance_changes (account_id, created_at DESC, id DESC);
CREATE INDEX account_balance_changes_account_resource_created_idx
    ON account_balance_changes (account_id, resource_address, created_at DESC, id DESC);
CREATE INDEX account_balance_changes_vault_idx ON account_balance_changes (vault_id);
CREATE INDEX account_balance_changes_transaction_idx ON account_balance_changes (transaction_id);
CREATE UNIQUE INDEX account_balance_changes_vault_transaction_uniq
    ON account_balance_changes (vault_id, transaction_id)
    WHERE transaction_id IS NOT NULL;
