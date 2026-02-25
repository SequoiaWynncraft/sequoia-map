use std::collections::HashMap;

use sequoia_shared::{Territory, TerritoryChange, TerritoryMap};

use crate::animation::ColorTransition;
use crate::colors::rgba_css;

/// Pre-formatted CSS rgba strings for the fixed set of alpha values used in rendering.
/// Avoids hundreds of `format!()` allocations per frame.
#[derive(Debug, Clone)]
pub struct CachedColors {
    /// Fill: normal (0.22)
    pub fill_normal: String,
    /// Fill: hovered (0.30)
    pub fill_hovered: String,
    /// Fill: selected (0.35)
    pub fill_selected: String,
    /// Border: normal (0.65)
    pub border_normal: String,
    /// Minimap fill (0.45)
    pub minimap_fill: String,
}

impl CachedColors {
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            fill_normal: rgba_css(r, g, b, 0.22),
            fill_hovered: rgba_css(r, g, b, 0.30),
            fill_selected: rgba_css(r, g, b, 0.35),
            border_normal: rgba_css(r, g, b, 0.65),
            minimap_fill: rgba_css(r, g, b, 0.45),
        }
    }
}

/// Client-side territory with animation state.
#[derive(Debug, Clone)]
pub struct ClientTerritory {
    pub territory: Territory,
    pub animation: Option<ColorTransition>,
    /// Pre-computed guild color (CRC32 hash), avoids recomputation per frame.
    pub guild_color: (u8, u8, u8),
    /// Pre-formatted CSS rgba strings for rendering.
    pub cached_colors: CachedColors,
}

impl ClientTerritory {
    pub fn from_territory(territory: Territory) -> Self {
        let guild_color = territory
            .guild
            .color
            .unwrap_or_else(|| sequoia_shared::guild_color(&territory.guild.name));
        let cached_colors = CachedColors::from_rgb(guild_color.0, guild_color.1, guild_color.2);
        Self {
            territory,
            animation: None,
            guild_color,
            cached_colors,
        }
    }
}

pub type ClientTerritoryMap = HashMap<String, ClientTerritory>;

/// Build client territory map from a full snapshot.
pub fn from_snapshot(map: TerritoryMap) -> ClientTerritoryMap {
    map.into_iter()
        .map(|(name, t)| (name, ClientTerritory::from_territory(t)))
        .collect()
}

/// Apply incremental changes to the client territory map.
/// `duration_ms` controls color transition length: 0 = instant (no animation object created).
pub fn apply_changes(
    territories: &mut ClientTerritoryMap,
    changes: &[TerritoryChange],
    now: f64,
    duration_ms: f64,
) {
    for change in changes {
        let old_color = territories.get(&change.territory).map(|ct| ct.guild_color);

        let new_color = change
            .guild
            .color
            .unwrap_or_else(|| sequoia_shared::guild_color(&change.guild.name));

        let acquired = chrono::DateTime::parse_from_rfc3339(&change.acquired)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());

        let new_territory = Territory {
            guild: sequoia_shared::GuildRef {
                uuid: change.guild.uuid.clone(),
                name: change.guild.name.clone(),
                prefix: change.guild.prefix.clone(),
                color: change.guild.color,
            },
            acquired,
            location: change.location.clone(),
            resources: change.resources.clone(),
            connections: change.connections.clone(),
        };

        let animation = if duration_ms > 0.0 {
            old_color.map(|from| ColorTransition::new(from, new_color, now, duration_ms))
        } else {
            None
        };

        let cached_colors = CachedColors::from_rgb(new_color.0, new_color.1, new_color.2);
        territories.insert(
            change.territory.clone(),
            ClientTerritory {
                territory: new_territory,
                animation,
                guild_color: new_color,
                cached_colors,
            },
        );
    }
}
