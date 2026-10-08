-- Requests for this wallet to co-sign a transaction that someone else seals.
--
-- A signing request is approved by a person and yields a TransactionSignature
-- that the requester assembles into a transaction and submits elsewhere.
--
-- Invariants:
--   * `unsigned_transaction` (JSON) and `seal_public_key` are immutable once
--     written. `message_hash` is the authorization message derived from them
--     at creation, and is exactly what the approval signs.
--   * `signer_public_key` is the public key of `key_id`, resolved at creation.
--   * `signature` is non-NULL exactly when `status` is 'Signed'.
--   * `memo` is requester-supplied free text, shown to the approver as such.
--   * `requester` (JSON `SigningRequester`) is who created the request: the
--     wallet session, an API key by its admin-assigned name, or an app
--     connected over WebRTC. Display and audit only.
--   * Expiry is derived on read: a row still 'Pending' past `expires_at` is
--     expired.
CREATE TABLE signing_requests (
    id                   INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    unsigned_transaction TEXT     NOT NULL,
    seal_public_key      TEXT     NOT NULL,
    key_id               TEXT     NOT NULL,
    signer_public_key    TEXT     NOT NULL,
    message_hash         TEXT     NOT NULL,
    memo                 TEXT     NOT NULL,
    requester            TEXT     NOT NULL,
    status               TEXT     NOT NULL,
    signature            TEXT     NULL,
    expires_at           DATETIME NOT NULL,
    decided_at           DATETIME NULL,
    created_at           DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at           DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX signing_requests_status_idx ON signing_requests (status);
