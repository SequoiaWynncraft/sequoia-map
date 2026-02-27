use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::Deserialize;
use sqlx::{Postgres, QueryBuilder};
use tracing::{info, warn};

use crate::config::{ATHENA_REFRESH_SECS, ATHENA_TERRITORY_URL};
use crate::state::AppState;

#[derive(Deserialize)]
struct AthenaResponse {
    territories: HashMap<String, AthenaTerritory>,
}

#[derive(Deserialize)]
struct AthenaTerritory {
    #[serde(default)]
    guild: Option<String>,
    #[serde(default, rename = "guildColor")]
    guild_color: Option<String>,
}

pub async fn run(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(ATHENA_REFRESH_SECS));

    loop {
        interval.tick().await;

        match fetch_guild_colors(&state.http_client).await {
            Ok(colors) => {
                let count = colors.len();
                if let Some(pool) = state.db.as_ref()
                    && let Err(e) = persist_guild_colors(pool, &colors).await
                {
                    warn!("failed to persist guild colors cache: {e}");
                }
                *state.guild_colors.write().await = colors;
                state.guild_colors_dirty.store(true, Ordering::Release);
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
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        let preview = String::from_utf8_lossy(&bytes)
            .chars()
            .take(200)
            .collect::<String>();
        return Err(format!("upstream status {status}; body preview: {preview}").into());
    }

    parse_athena_guild_colors_payload(bytes.as_ref())
        .map_err(|e| format!("failed to decode Athena payload: {e}").into())
}

fn parse_athena_guild_colors_payload(
    bytes: &[u8],
) -> Result<HashMap<String, (u8, u8, u8)>, serde_json::Error> {
    let data: AthenaResponse = serde_json::from_slice(bytes)?;
    let mut colors = HashMap::new();
    for entry in data.territories.values() {
        let Some(guild_name) = entry
            .guild
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        let Some(guild_color_hex) = entry.guild_color.as_deref() else {
            continue;
        };
        if let Some(rgb) = parse_hex_color(guild_color_hex) {
            colors.entry(guild_name.to_string()).or_insert(rgb);
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

async fn persist_guild_colors(
    pool: &sqlx::PgPool,
    colors: &HashMap<String, (u8, u8, u8)>,
) -> Result<(), String> {
    if colors.is_empty() {
        return Ok(());
    }

    let mut query_builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO guild_color_cache (guild_name, color_r, color_g, color_b) ",
    );
    query_builder.push_values(colors.iter(), |mut builder, (guild_name, color)| {
        builder
            .push_bind(guild_name)
            .push_bind(i16::from(color.0))
            .push_bind(i16::from(color.1))
            .push_bind(i16::from(color.2));
    });
    query_builder.push(
        " ON CONFLICT (guild_name) DO UPDATE \
         SET color_r = EXCLUDED.color_r, \
             color_g = EXCLUDED.color_g, \
             color_b = EXCLUDED.color_b, \
             updated_at = now()",
    );

    query_builder
        .build()
        .execute(pool)
        .await
        .map_err(|e| format!("upsert guild color cache rows: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_athena_guild_colors_payload, parse_hex_color};

    #[test]
    fn parse_hex_color_accepts_valid_hex_triplets() {
        assert_eq!(parse_hex_color("#ffd700"), Some((255, 215, 0)));
        assert_eq!(parse_hex_color("50c878"), Some((80, 200, 120)));
    }

    #[test]
    fn parse_athena_payload_tolerates_null_guild_rows() {
        let payload = r##"{
            "territories": {
                "Lion Lair": {
                    "territory": "Lion Lair",
                    "guild": null,
                    "guildPrefix": null,
                    "guildColor": "#ffffff",
                    "acquired": "2026-02-26T22:13:13.493000Z",
                    "location": {"startX": 890, "startZ": -2140, "endX": 790, "endZ": -2320}
                },
                "Ragni": {
                    "territory": "Ragni",
                    "guild": "Aequitas",
                    "guildPrefix": "Aeq",
                    "guildColor": "#ffd700",
                    "acquired": "2026-02-26T17:20:41.785000Z",
                    "location": {"startX": -955, "startZ": -1415, "endX": -756, "endZ": -1748}
                }
            }
        }"##;

        let colors = parse_athena_guild_colors_payload(payload.as_bytes())
            .expect("payload should decode despite null guild rows");
        assert_eq!(colors.len(), 1);
        assert_eq!(colors.get("Aequitas"), Some(&(255, 215, 0)));
    }
}
