-- Disable foreign key constraints temporarily to allow for table alteration
PRAGMA foreign_keys = OFF;

-- Convert vault_locks.amount from integer to string
ALTER TABLE vault_locks
    RENAME TO vault_locks_old;

CREATE TABLE vault_locks
(
    id         INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    vault_id   INTEGER  NOT NULL REFERENCES vaults (id) ON DELETE CASCADE,
    lock_id    INTEGER  NOT NULL REFERENCES locks (id) ON DELETE CASCADE,
    amount     TEXT     NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO vault_locks (id, vault_id, lock_id, amount, created_at)
SELECT id, vault_id, lock_id, CAST(amount AS TEXT), created_at
FROM vault_locks_old;

DROP TABLE vault_locks_old;

-- Re-enable foreign key constraints
PRAGMA foreign_keys = ON;