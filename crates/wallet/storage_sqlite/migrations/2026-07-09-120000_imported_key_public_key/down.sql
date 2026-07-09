DROP INDEX key_manager_imported_keys_uniq_public_key;

ALTER TABLE key_manager_imported_keys
    DROP COLUMN public_key;

CREATE UNIQUE INDEX key_manager_imported_keys_uniq_label
    ON key_manager_imported_keys (label);
