ALTER TABLE territory_events
    ADD COLUMN guild_color_r SMALLINT,
    ADD COLUMN guild_color_g SMALLINT,
    ADD COLUMN guild_color_b SMALLINT,
    ADD COLUMN prev_guild_color_r SMALLINT,
    ADD COLUMN prev_guild_color_g SMALLINT,
    ADD COLUMN prev_guild_color_b SMALLINT;

ALTER TABLE territory_events
    ADD CONSTRAINT chk_territory_events_guild_color_r_range
        CHECK (guild_color_r IS NULL OR (guild_color_r >= 0 AND guild_color_r <= 255)),
    ADD CONSTRAINT chk_territory_events_guild_color_g_range
        CHECK (guild_color_g IS NULL OR (guild_color_g >= 0 AND guild_color_g <= 255)),
    ADD CONSTRAINT chk_territory_events_guild_color_b_range
        CHECK (guild_color_b IS NULL OR (guild_color_b >= 0 AND guild_color_b <= 255)),
    ADD CONSTRAINT chk_territory_events_prev_guild_color_r_range
        CHECK (
            prev_guild_color_r IS NULL OR (prev_guild_color_r >= 0 AND prev_guild_color_r <= 255)
        ),
    ADD CONSTRAINT chk_territory_events_prev_guild_color_g_range
        CHECK (
            prev_guild_color_g IS NULL OR (prev_guild_color_g >= 0 AND prev_guild_color_g <= 255)
        ),
    ADD CONSTRAINT chk_territory_events_prev_guild_color_b_range
        CHECK (
            prev_guild_color_b IS NULL OR (prev_guild_color_b >= 0 AND prev_guild_color_b <= 255)
        );
