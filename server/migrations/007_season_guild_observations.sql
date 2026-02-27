CREATE TABLE season_guild_observations (
    observed_at TIMESTAMPTZ NOT NULL,
    season_id INTEGER NOT NULL,
    guild_name TEXT NOT NULL,
    guild_uuid TEXT,
    guild_prefix TEXT,
    territory_count SMALLINT NOT NULL,
    season_rating INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (observed_at, guild_name)
);

CREATE INDEX idx_season_guild_obs_guild_observed_desc
    ON season_guild_observations (guild_name, observed_at DESC);

CREATE INDEX idx_season_guild_obs_season_observed_desc
    ON season_guild_observations (season_id, observed_at DESC);
