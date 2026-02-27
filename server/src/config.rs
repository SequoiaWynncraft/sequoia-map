use std::time::Duration;

pub const WYNNCRAFT_TERRITORY_URL: &str = "https://api.wynncraft.com/v3/guild/list/territory";
pub const WYNNCRAFT_GUILD_URL: &str = "https://api.wynncraft.com/v3/guild";

pub const TERREXTRA_URL: &str = "https://gist.githubusercontent.com/Zatzou/14c82f2df0eb4093dfa1d543b78a73a8/raw/d03273fce33c031498c07e21b94f17644c8aae98/terrextra.json";
pub const TERREXTRA_REFRESH_SECS: u64 = 3600; // re-fetch hourly

pub const ATHENA_TERRITORY_URL: &str = "https://athena.wynntils.com/cache/get/territoryList";
pub const ATHENA_REFRESH_SECS: u64 = 600; // 10 minutes

pub const POLL_INTERVAL_SECS: u64 = 10;
pub const GUILD_CACHE_TTL_SECS: i64 = 600; // 10 minutes
pub const DEFAULT_GUILDS_ONLINE_CACHE_TTL_SECS: i64 = 120; // 2 minutes
pub const DEFAULT_GUILDS_ONLINE_MAX_CONCURRENCY: usize = 8;
pub const MAX_GUILD_CACHE_ENTRIES: usize = 64;
pub const SSE_KEEPALIVE_SECS: u64 = 15;
pub const DEFAULT_BROADCAST_BUFFER: usize = 256;
pub const DEFAULT_DB_MAX_CONNECTIONS: u32 = 10;
pub const DEFAULT_UPSTREAM_HTTP_TIMEOUT_SECS: u64 = 10;
pub const DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS: u64 = 3;
pub const SERVER_PORT: u16 = 3000;

// History feature
pub const SNAPSHOT_INTERVAL_SECS: u64 = 21600; // every 6 hours
pub const RETENTION_DAYS: i64 = 365;
pub const RETENTION_CHECK_SECS: u64 = 86400; // daily

pub fn seq_live_handoff_v1_enabled() -> bool {
    std::env::var("SEQ_LIVE_HANDOFF_V1")
        .or_else(|_| std::env::var("seq_live_handoff_v1"))
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(true)
}

pub fn db_max_connections() -> u32 {
    std::env::var("DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_DB_MAX_CONNECTIONS)
}

pub fn sse_broadcast_buffer() -> usize {
    std::env::var("SSE_BROADCAST_BUFFER")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_BROADCAST_BUFFER)
}

pub fn upstream_http_timeout() -> Duration {
    std::env::var("UPSTREAM_HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_UPSTREAM_HTTP_TIMEOUT_SECS))
}

pub fn upstream_connect_timeout() -> Duration {
    std::env::var("UPSTREAM_CONNECT_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS))
}

pub fn guilds_online_cache_ttl_secs() -> i64 {
    std::env::var("GUILDS_ONLINE_CACHE_TTL_SECS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_GUILDS_ONLINE_CACHE_TTL_SECS)
}

pub fn guilds_online_max_concurrency() -> usize {
    std::env::var("GUILDS_ONLINE_MAX_CONCURRENCY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_GUILDS_ONLINE_MAX_CONCURRENCY)
}
