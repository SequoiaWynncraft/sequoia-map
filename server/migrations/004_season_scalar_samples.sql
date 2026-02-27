CREATE TABLE season_scalar_samples (
    sampled_at      TIMESTAMPTZ NOT NULL,
    season_id       INTEGER NOT NULL,
    scalar_weighted DOUBLE PRECISION NOT NULL,
    scalar_raw      DOUBLE PRECISION NOT NULL,
    confidence      DOUBLE PRECISION NOT NULL,
    sample_count    INTEGER NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_season_scalar_sampled_at_desc
    ON season_scalar_samples (sampled_at DESC);

CREATE INDEX idx_season_scalar_season_sampled_desc
    ON season_scalar_samples (season_id, sampled_at DESC);
