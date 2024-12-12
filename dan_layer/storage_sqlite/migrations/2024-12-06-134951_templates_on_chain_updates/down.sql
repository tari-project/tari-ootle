ALTER TABLE templates DROP COLUMN author_public_key;
ALTER TABLE templates DROP COLUMN url;

ALTER TABLE templates ADD COLUMN wasm_path VARCHAR(255) NULL;
ALTER TABLE templates ADD COLUMN url TEXT NOT NULL;
ALTER TABLE templates ADD COLUMN height bigint NOT NULL;


