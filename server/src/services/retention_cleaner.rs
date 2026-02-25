use std::time::Duration;

use tracing::{info, warn};

use crate::config::{RETENTION_CHECK_SECS, RETENTION_DAYS};
use crate::state::AppState;

const BATCH_SIZE: i64 = 10_000;

/// Daily cleanup of old history data beyond the retention period.
pub async fn run(state: AppState) {
    let Some(pool) = state.db.as_ref().cloned() else {
        warn!("retention cleaner disabled: no database configured");
        return;
    };

    info!(
        "Retention cleaner started (retention: {}d, check interval: {}s)",
        RETENTION_DAYS, RETENTION_CHECK_SECS
    );

    run_cleanup_once(&pool).await;

    let mut interval = tokio::time::interval(Duration::from_secs(RETENTION_CHECK_SECS));
    // Consume immediate tick so subsequent cleanup runs after the configured interval.
    interval.tick().await;

    loop {
        interval.tick().await;
        run_cleanup_once(&pool).await;
    }
}

async fn run_cleanup_once(pool: &sqlx::PgPool) {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(RETENTION_DAYS);

    // Delete old events in batches to avoid long locks
    let mut total_events = 0i64;
    loop {
        match sqlx::query(
            "DELETE FROM territory_events WHERE id IN \
             (SELECT id FROM territory_events WHERE recorded_at < $1 LIMIT $2)",
        )
        .bind(cutoff)
        .bind(BATCH_SIZE)
        .execute(pool)
        .await
        {
            Ok(result) => {
                let deleted = result.rows_affected() as i64;
                total_events += deleted;
                if deleted < BATCH_SIZE {
                    break;
                }
            }
            Err(e) => {
                warn!("Failed to delete old events: {e}");
                break;
            }
        }
    }

    // Delete old snapshots in batches
    let mut total_snapshots = 0i64;
    loop {
        match sqlx::query(
            "DELETE FROM territory_snapshots WHERE id IN \
             (SELECT id FROM territory_snapshots WHERE created_at < $1 LIMIT $2)",
        )
        .bind(cutoff)
        .bind(BATCH_SIZE)
        .execute(pool)
        .await
        {
            Ok(result) => {
                let deleted = result.rows_affected() as i64;
                total_snapshots += deleted;
                if deleted < BATCH_SIZE {
                    break;
                }
            }
            Err(e) => {
                warn!("Failed to delete old snapshots: {e}");
                break;
            }
        }
    }

    if total_events > 0 || total_snapshots > 0 {
        info!(
            "Retention cleanup: removed {total_events} events, {total_snapshots} snapshots older than {RETENTION_DAYS}d"
        );
    }
}
