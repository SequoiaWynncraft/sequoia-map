CREATE TABLE territory_events (
    id             BIGSERIAL PRIMARY KEY,
    recorded_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    acquired_at    TIMESTAMPTZ NOT NULL,
    territory      TEXT NOT NULL,
    guild_uuid     TEXT NOT NULL,
    guild_name     TEXT NOT NULL,
    guild_prefix   TEXT NOT NULL,
    prev_guild_uuid   TEXT,
    prev_guild_name   TEXT,
    prev_guild_prefix TEXT
);

CREATE INDEX idx_events_recorded ON territory_events (recorded_at);
CREATE INDEX idx_events_territory ON territory_events (territory, recorded_at);

CREATE TABLE territory_snapshots (
    id          BIGSERIAL PRIMARY KEY,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    ownership   JSONB NOT NULL
);

CREATE INDEX idx_snapshots_created ON territory_snapshots (created_at);
