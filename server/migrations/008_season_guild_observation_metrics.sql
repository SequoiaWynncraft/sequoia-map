ALTER TABLE season_guild_observations
    ADD COLUMN sr_gain_5m INTEGER,
    ADD COLUMN sample_rank INTEGER;

CREATE INDEX idx_season_guild_observations_observed_desc
    ON season_guild_observations (observed_at DESC);

CREATE INDEX idx_season_guild_observations_name_observed_desc
    ON season_guild_observations (guild_name, observed_at DESC);
