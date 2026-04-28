DROP INDEX IF EXISTS events_resource_address_idx;
ALTER TABLE events DROP COLUMN resource_address;
