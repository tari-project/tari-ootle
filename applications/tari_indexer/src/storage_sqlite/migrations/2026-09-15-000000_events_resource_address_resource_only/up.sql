-- `events.resource_address` now means "the resource this event's substate_id names",
-- which is the `std.resource.*` family alone. It previously also carried the
-- `resource_address` payload entry that `std.vault.deposit` / `std.vault.withdraw`
-- used to emit.
--
-- Rows written under the old rule must be cleared, or the column means two different
-- things either side of this upgrade. An SSE subscription filtered by resource replays
-- from this column and then goes live against the in-memory filter, so a mixed table
-- makes the stream change meaning at the replay boundary with no error.
--
-- Only a resource substate_id renders with the `resource_` prefix. The underscore is
-- escaped because LIKE reads it as a single-character wildcard.
UPDATE events
SET resource_address = NULL
WHERE resource_address IS NOT NULL
  AND (substate_id IS NULL OR substate_id NOT LIKE 'resource\_%' ESCAPE '\');
