-- Indexes for SSE event catch-up queries which filter by topic and substate_id
-- and order by id ascending.
CREATE INDEX events_topic_idx ON events (topic);
CREATE INDEX events_substate_id_idx ON events (substate_id);
