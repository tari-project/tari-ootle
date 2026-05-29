-- Links a submitted transaction to the wallet account(s) it involves, so the
-- transactions list can be filtered per account.
--
-- A transaction may involve more than one owned account (e.g. a transfer from
-- account A to the wallet's own account B tags both), hence a join table rather
-- than a column on `transactions`. Tagging is done explicitly at submission time
-- by the handler that knows the involved accounts; stealth transactions are not
-- identifiable from on-chain data, so this is the only reliable linkage.
--
-- Only non-dry-run, wallet-submitted transactions are linked. Received funds are
-- discovered by scanning (not yet implemented) and are not represented here.
CREATE TABLE transaction_accounts (
    id             INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    transaction_id TEXT    NOT NULL REFERENCES transactions (transaction_id) ON DELETE CASCADE,
    account_id     INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at     DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT transaction_accounts_uniq UNIQUE (transaction_id, account_id)
);

CREATE INDEX transaction_accounts_account_idx ON transaction_accounts (account_id);
