use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use tracing::{info, warn};

use crate::config::{ATHENA_REFRESH_SECS, ATHENA_TERRITORY_URL};
use crate::state::AppState;

#[derive(Deserialize)]
struct AthenaResponse {
    territories: HashMap<String, AthenaTerritory>,
}

#[derive(Deserialize)]
struct AthenaTerritory {
    guild: String,
    #[serde(rename = "guildColor")]
    guild_color: String,
}

pub async fn run(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(ATHENA_REFRESH_SECS));

    loop {
        interval.tick().await;

        match fetch_guild_colors(&state.http_client).await {
            Ok(colors) => {
                let count = colors.len();
                *state.guild_colors.write().await = colors;
                info!("loaded guild colors for {count} guilds from Athena");
            }
            Err(e) => {
                warn!("failed to fetch guild colors from Athena: {e}");
            }
        }
    }
}

async fn fetch_guild_colors(
    client: &reqwest::Client,
) -> Result<HashMap<String, (u8, u8, u8)>, Box<dyn std::error::Error + Send + Sync>> {
    let resp = client.get(ATHENA_TERRITORY_URL).send().await?;
    let data: AthenaResponse = resp.json().await?;

    let mut colors = HashMap::new();
    for entry in data.territories.values() {
        if let Some(rgb) = parse_hex_color(&entry.guild_color) {
            colors.entry(entry.guild.clone()).or_insert(rgb);
        }
    }

    Ok(colors)
}

fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}
