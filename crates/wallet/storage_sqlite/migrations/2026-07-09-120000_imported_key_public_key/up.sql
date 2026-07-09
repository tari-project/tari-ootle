-- The public key derived from the imported secret is the stable identity of an imported key; the label is a
-- mutable display name. Key uniqueness/idempotency is therefore on the public key, not the label.
DROP INDEX key_manager_imported_keys_uniq_label;

ALTER TABLE key_manager_imported_keys
    ADD COLUMN public_key TEXT NOT NULL DEFAULT '';

CREATE UNIQUE INDEX key_manager_imported_keys_uniq_public_key
    ON key_manager_imported_keys (public_key);
