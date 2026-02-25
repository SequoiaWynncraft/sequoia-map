use std::time::Duration;

use chrono::Utc;
use tracing::info;

use crate::config::GUILD_CACHE_TTL_SECS;
use crate::state::AppState;

const EVICTION_INTERVAL_SECS: u64 = 300; // 5 minutes

pub async fn run(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(EVICTION_INTERVAL_SECS));

    loop {
        interval.tick().await;

        let before = state.guild_cache.len();
        let now = Utc::now();

        state.guild_cache.retain(|_, cached| {
            now.signed_duration_since(cached.cached_at).num_seconds() < GUILD_CACHE_TTL_SECS
        });

        let evicted = before - state.guild_cache.len();
        if evicted > 0 {
            info!(
                "evicted {evicted} stale guild cache entries ({} remaining)",
                state.guild_cache.len()
            );
        }
    }
}
