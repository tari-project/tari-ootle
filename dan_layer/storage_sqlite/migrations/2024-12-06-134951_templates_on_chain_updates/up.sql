ALTER TABLE templates DROP COLUMN wasm_path;
ALTER TABLE templates DROP COLUMN url;
ALTER TABLE templates DROP COLUMN height;

ALTER TABLE templates ADD COLUMN author_public_key BLOB NOT NULL;
ALTER TABLE templates ADD COLUMN url TEXT NULL;
