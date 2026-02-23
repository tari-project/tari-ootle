-- Remove ownerkey column from the resources table
ALTER TABLE resources
    DROP COLUMN owner_key;