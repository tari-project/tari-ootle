-- Store the resource address (when present) an event refers to, so SSE
-- consumers can filter a stream down to a single token's activity server-side.
--
-- Populated from event.substate_id for `std.resource.*` events, whose substate_id
-- is the resource address. Other events, vault events included, leave it NULL:
-- their substate_id names something else and no event payload is trusted for it.
ALTER TABLE events ADD COLUMN resource_address TEXT NULL;
CREATE INDEX events_resource_address_idx ON events (resource_address);
