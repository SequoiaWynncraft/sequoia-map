use std::time::Duration;

use tracing::{info, warn};

use crate::config::SNAPSHOT_INTERVAL_SECS;
use crate::state::AppState;

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
    let (territory_count, ownership_json) = {
        let snapshot = state.live_snapshot.read().await;
        if snapshot.territories.is_empty() {
            return;
        }
        (snapshot.territories.len(), snapshot.ownership_json.clone())
    };
    let Ok(ownership_json_str) = std::str::from_utf8(ownership_json.as_ref()) else {
        warn!("Failed to decode pre-serialized ownership snapshot as UTF-8");
        return;
    };

    match sqlx::query("INSERT INTO territory_snapshots (ownership) VALUES ($1::jsonb)")
        .bind(ownership_json_str)
        .execute(pool)
        .await
    {
        Ok(_) => info!("Saved ownership snapshot ({} territories)", territory_count),
        Err(e) => warn!("Failed to insert snapshot: {e}"),
    }
}
