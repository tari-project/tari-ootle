-- Store the resource address (when present) an event refers to, so SSE
-- consumers can filter a stream down to a single token's activity server-side.
--
-- Populated from event.substate_id for `std.resource.*` events (the substate_id
-- is the resource address), or from the `resource_address` payload entry for
-- `std.vault.deposit` / `std.vault.withdraw`.
ALTER TABLE events ADD COLUMN resource_address TEXT NULL;
CREATE INDEX events_resource_address_idx ON events (resource_address);
