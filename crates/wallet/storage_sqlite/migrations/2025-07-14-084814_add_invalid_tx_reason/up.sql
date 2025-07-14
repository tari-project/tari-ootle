-- Your SQL goes here
ALTER TABLE transactions
    ADD COLUMN invalid_reason TEXT NULL;