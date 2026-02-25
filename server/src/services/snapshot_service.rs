use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::config::SNAPSHOT_INTERVAL_SECS;
use crate::state::AppState;

/// Compact ownership record stored in snapshot JSONB.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnershipEntry {
    guild_uuid: String,
    guild_name: String,
    guild_prefix: String,
    acquired_at: String,
}

/// Periodically takes ownership snapshots for efficient historical reconstruction.
pub async fn run(state: AppState) {
    let Some(pool) = state.db.as_ref().cloned() else {
        warn!("snapshot service disabled: no database configured");
        return;
    };

    info!(
        "Snapshot service started (interval: {}s)",
        SNAPSHOT_INTERVAL_SECS
    );

    run_snapshot_once(&state, &pool).await;

    let mut interval = tokio::time::interval(Duration::from_secs(SNAPSHOT_INTERVAL_SECS));
    // Consume the immediate first tick so we wait a full interval after startup snapshot.
    interval.tick().await;

    loop {
        interval.tick().await;
        run_snapshot_once(&state, &pool).await;
    }
}

async fn run_snapshot_once(state: &AppState, pool: &sqlx::PgPool) {
    let territories = state.live_snapshot.read().await;
    if territories.territories.is_empty() {
        return;
    }

    // Build compact ownership map (strip resources/connections/location)
    let ownership: HashMap<String, OwnershipEntry> = territories
        .territories
        .iter()
        .map(|(name, terr)| {
            (
                name.clone(),
                OwnershipEntry {
                    guild_uuid: terr.guild.uuid.clone(),
                    guild_name: terr.guild.name.clone(),
                    guild_prefix: terr.guild.prefix.clone(),
                    acquired_at: terr.acquired.to_rfc3339(),
                },
            )
        })
        .collect();
    drop(territories);

    let ownership_json = match serde_json::to_value(&ownership) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to serialize ownership snapshot: {e}");
            return;
        }
    };

    match sqlx::query("INSERT INTO territory_snapshots (ownership) VALUES ($1)")
        .bind(&ownership_json)
        .execute(pool)
        .await
    {
        Ok(_) => info!("Saved ownership snapshot ({} territories)", ownership.len()),
        Err(e) => warn!("Failed to insert snapshot: {e}"),
    }
}
