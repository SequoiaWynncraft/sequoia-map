ALTER TABLE territory_events
    ADD COLUMN stream_seq BIGINT;

WITH ordered_events AS (
    SELECT id, ROW_NUMBER() OVER (ORDER BY recorded_at ASC, id ASC) AS seq
    FROM territory_events
)
UPDATE territory_events e
SET stream_seq = ordered_events.seq
FROM ordered_events
WHERE e.id = ordered_events.id;

ALTER TABLE territory_events
    ALTER COLUMN stream_seq SET NOT NULL;

CREATE UNIQUE INDEX idx_events_stream_seq_unique ON territory_events (stream_seq);
CREATE INDEX idx_events_stream_seq ON territory_events (stream_seq);
