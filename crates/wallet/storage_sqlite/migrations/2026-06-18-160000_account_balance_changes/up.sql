CREATE TABLE account_balance_changes
(
    id                  INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    account_id          INTEGER  NOT NULL,
    resource_id         INTEGER  NOT NULL,
    account_address     TEXT     NOT NULL,
    vault_address       TEXT     NULL,
    vault_version       BIGINT   NULL,
    resource_address    TEXT     NOT NULL,
    token_symbol        TEXT     NULL,
    divisibility        INTEGER  NOT NULL,
    source_type         TEXT     NOT NULL CHECK (source_type IN ('transaction', 'scan', 'recovery')),
    transaction_id      TEXT     NULL,
    revealed_before     TEXT     NOT NULL,
    revealed_after      TEXT     NOT NULL,
    confidential_before TEXT     NOT NULL,
    confidential_after  TEXT     NOT NULL,
    created_at          DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (
        (source_type = 'transaction' AND transaction_id IS NOT NULL) OR
        (source_type IN ('scan', 'recovery') AND transaction_id IS NULL)
    ),
    CHECK (
        (vault_address IS NULL AND vault_version IS NULL) OR
        (vault_address IS NOT NULL AND vault_version IS NOT NULL)
    ),
    CHECK (revealed_before <> revealed_after OR confidential_before <> confidential_after),
    CHECK (divisibility BETWEEN 0 AND 255)
);

CREATE INDEX account_balance_changes_account_created_idx
    ON account_balance_changes (account_id, created_at DESC, id DESC);
CREATE INDEX account_balance_changes_account_resource_created_idx
    ON account_balance_changes (account_id, resource_id, created_at DESC, id DESC);
CREATE INDEX account_balance_changes_vault_idx ON account_balance_changes (vault_address);
CREATE INDEX account_balance_changes_transaction_idx ON account_balance_changes (transaction_id);
CREATE UNIQUE INDEX account_balance_changes_account_resource_transaction_uniq
    ON account_balance_changes (account_id, resource_id, transaction_id)
    WHERE transaction_id IS NOT NULL;
CREATE UNIQUE INDEX account_balance_changes_vault_version_uniq
    ON account_balance_changes (vault_address, vault_version)
    WHERE vault_address IS NOT NULL AND vault_version IS NOT NULL;
