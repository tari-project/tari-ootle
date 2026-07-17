-- Transaction requests created by a limited-permission tool, approved by a
-- separately-permissioned principal, and only then submitted (issue #2343).
--
-- Why a table of its own rather than columns on `transactions`:
--   * `transactions.status` tracks the consensus lifecycle. A request that is
--     never approved never becomes a transaction at all, so it has no place
--     in that lifecycle.
--   * A request is the artifact a human authorises. It must be durable across
--     restarts, which rules out the in-memory `RefreshTokenStore` shape.
--
-- Invariants:
--   * `unsigned_transaction` is the transaction frozen at creation, stored as
--     JSON like `transactions.transaction_json`: inputs already detected and
--     `is_seal_signer_authorized` already settled to the value `seal()` will
--     produce. It is immutable once written, so the approver views exactly what
--     submit seals.
--   * `seal_signer` / `other_signers` are the caller's choice of who pays and
--     co-signs. Stealth spend keys are deliberately absent: they are derived
--     at submit from the `lock_ids` below, so a caller cannot ask the wallet
--     to sign with a key of its choosing.
--   * `requested_by` is the admin-assigned name of the API key that created
--     the request, or NULL for a wallet session. Display and audit only --
--     nothing authorises on it, and no creator check gates approval.
--   * `expires_at` bounds the approval window. Expiry is *derived* on read
--     rather than written by a reaper: a row still marked Pending past this
--     timestamp is expired. The locks referenced by `lock_ids` are extended
--     past this deadline at creation, so the lock always outlives the request.
CREATE TABLE transaction_requests (
    id                   INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    unsigned_transaction TEXT     NOT NULL,
    seal_signer          TEXT     NOT NULL,
    other_signers        TEXT     NOT NULL,
    signatures           TEXT     NOT NULL,
    lock_ids             TEXT     NOT NULL,
    requested_by         TEXT     NULL,
    status               TEXT     NOT NULL,
    transaction_id       TEXT     NULL,
    expires_at           DATETIME NOT NULL,
    approved_at          DATETIME NULL,
    created_at           DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at           DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX transaction_requests_status_idx ON transaction_requests (status);
