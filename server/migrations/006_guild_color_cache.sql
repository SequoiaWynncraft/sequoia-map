CREATE TABLE guild_color_cache (
    guild_name TEXT PRIMARY KEY,
    color_r SMALLINT NOT NULL,
    color_g SMALLINT NOT NULL,
    color_b SMALLINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_guild_color_cache_r_range CHECK (color_r >= 0 AND color_r <= 255),
    CONSTRAINT chk_guild_color_cache_g_range CHECK (color_g >= 0 AND color_g <= 255),
    CONSTRAINT chk_guild_color_cache_b_range CHECK (color_b >= 0 AND color_b <= 255)
);

CREATE INDEX idx_guild_color_cache_updated_at_desc
    ON guild_color_cache (updated_at DESC);
