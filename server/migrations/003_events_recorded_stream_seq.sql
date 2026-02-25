CREATE INDEX idx_events_recorded_stream_seq
    ON territory_events (recorded_at, stream_seq);
