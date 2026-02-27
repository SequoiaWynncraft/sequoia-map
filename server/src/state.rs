use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use sequoia_shared::{LiveState, Resources, SeasonScalarSample, TerritoryMap};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::{RwLock, broadcast};
use tracing::warn;

use crate::config::{
    seq_live_handoff_v1_enabled, sse_broadcast_buffer, upstream_connect_timeout,
    upstream_http_timeout,
};

pub type GuildColor = (u8, u8, u8);
pub type GuildColorMap = HashMap<String, GuildColor>;
pub type CachedScalarSample = (SeasonScalarSample, Arc<Bytes>);

/// Pre-serialized SSE event — serialized once by the poller, shared by all clients via Arc.
#[derive(Debug, Clone)]
pub enum PreSerializedEvent {
    Snapshot { seq: u64, json: Arc<Bytes> },
    Update { seq: u64, json: Arc<Bytes> },
}

#[derive(Debug, Clone)]
pub struct LiveSnapshot {
    pub seq: u64,
    pub timestamp: String,
    pub territories: TerritoryMap,
    pub snapshot_json: Arc<Bytes>,
    pub territories_json: Arc<Bytes>,
    pub live_state_json: Arc<Bytes>,
    pub ownership_json: Arc<Bytes>,
}

impl Default for LiveSnapshot {
    fn default() -> Self {
        let seq = 0;
        let timestamp = Utc::now().to_rfc3339();
        let territories = TerritoryMap::new();
        let live_state_json = serde_json::to_vec(&LiveState {
            seq,
            timestamp: timestamp.clone(),
            territories: territories.clone(),
        })
        .map(Bytes::from)
        .unwrap_or_else(|_| Bytes::from_static(br#"{"seq":0,"timestamp":"","territories":{}}"#));

        Self {
            seq,
            timestamp,
            territories,
            snapshot_json: Arc::new(Bytes::new()),
            territories_json: Arc::new(Bytes::from_static(b"{}")),
            live_state_json: Arc::new(live_state_json),
            ownership_json: Arc::new(Bytes::from_static(b"{}")),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExtraTerrInfo {
    #[serde(default)]
    pub resources: Resources,
    #[serde(default)]
    pub connections: Vec<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub live_snapshot: Arc<RwLock<LiveSnapshot>>,
    pub next_seq: Arc<AtomicU64>,
    pub event_tx: broadcast::Sender<PreSerializedEvent>,
    pub guild_cache: Arc<DashMap<String, CachedGuild>>,
    /// Extra territory data (resources, connections) from supplemental gist.
    pub extra_terr: Arc<RwLock<HashMap<String, ExtraTerrInfo>>>,
    pub extra_data_dirty: Arc<AtomicBool>,
    /// Guild name -> RGB color from Athena/Wynntils.
    pub guild_colors: Arc<RwLock<GuildColorMap>>,
    pub guild_colors_dirty: Arc<AtomicBool>,
    /// Latest computed season scalar sample and pre-serialized API payload.
    pub latest_scalar_sample: Arc<RwLock<Option<CachedScalarSample>>>,
    pub http_client: reqwest::Client,
    /// PostgreSQL pool for history persistence. None if DATABASE_URL is not set.
    pub db: Option<PgPool>,
    pub seq_live_handoff_v1: bool,
    pub observability: Arc<ObservabilityCounters>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedGuild {
    pub data: String,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub struct ObservabilityCounters {
    live_state_requests_total: AtomicU64,
    persist_failures_total: AtomicU64,
    dropped_update_events_total: AtomicU64,
    persisted_update_events_total: AtomicU64,
    guilds_online_requests_total: AtomicU64,
    guilds_online_cache_hits_total: AtomicU64,
    guilds_online_cache_misses_total: AtomicU64,
    guilds_online_upstream_errors_total: AtomicU64,
}

#[derive(Debug, Clone, Copy)]
pub struct ObservabilitySnapshot {
    pub live_state_requests_total: u64,
    pub persist_failures_total: u64,
    pub dropped_update_events_total: u64,
    pub persisted_update_events_total: u64,
    pub guilds_online_requests_total: u64,
    pub guilds_online_cache_hits_total: u64,
    pub guilds_online_cache_misses_total: u64,
    pub guilds_online_upstream_errors_total: u64,
}

impl ObservabilityCounters {
    pub fn snapshot(&self) -> ObservabilitySnapshot {
        ObservabilitySnapshot {
            live_state_requests_total: self.live_state_requests_total.load(Ordering::Relaxed),
            persist_failures_total: self.persist_failures_total.load(Ordering::Relaxed),
            dropped_update_events_total: self.dropped_update_events_total.load(Ordering::Relaxed),
            persisted_update_events_total: self
                .persisted_update_events_total
                .load(Ordering::Relaxed),
            guilds_online_requests_total: self.guilds_online_requests_total.load(Ordering::Relaxed),
            guilds_online_cache_hits_total: self
                .guilds_online_cache_hits_total
                .load(Ordering::Relaxed),
            guilds_online_cache_misses_total: self
                .guilds_online_cache_misses_total
                .load(Ordering::Relaxed),
            guilds_online_upstream_errors_total: self
                .guilds_online_upstream_errors_total
                .load(Ordering::Relaxed),
        }
    }

    pub fn record_live_state_request(&self) {
        self.live_state_requests_total
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_persist_failure(&self) {
        self.persist_failures_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_dropped_update_events(&self, count: u64) {
        self.dropped_update_events_total
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_persisted_update_events(&self, count: u64) {
        self.persisted_update_events_total
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_guilds_online_request(&self) {
        self.guilds_online_requests_total
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_guilds_online_cache_hits(&self, count: u64) {
        self.guilds_online_cache_hits_total
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_guilds_online_cache_misses(&self, count: u64) {
        self.guilds_online_cache_misses_total
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_guilds_online_upstream_errors(&self, count: u64) {
        self.guilds_online_upstream_errors_total
            .fetch_add(count, Ordering::Relaxed);
    }
}

impl AppState {
    pub fn new(db: Option<PgPool>) -> Self {
        let (event_tx, _) = broadcast::channel(sse_broadcast_buffer());
        let request_timeout = upstream_http_timeout();
        let connect_timeout = upstream_connect_timeout();
        let http_client = reqwest::Client::builder()
            .user_agent("sequoia-map/0.1")
            .timeout(request_timeout)
            .connect_timeout(connect_timeout)
            .build()
            .or_else(|e| {
                warn!(
                    error = %e,
                    "failed to build configured HTTP client, retrying without custom user-agent"
                );
                reqwest::Client::builder()
                    .timeout(request_timeout)
                    .connect_timeout(connect_timeout)
                    .build()
            })
            .unwrap_or_else(|e| {
                panic!("failed to build timeout-configured HTTP client: {e}");
            });
        Self {
            live_snapshot: Arc::new(RwLock::new(LiveSnapshot::default())),
            next_seq: Arc::new(AtomicU64::new(0)),
            event_tx,
            guild_cache: Arc::new(DashMap::new()),
            extra_terr: Arc::new(RwLock::new(HashMap::new())),
            extra_data_dirty: Arc::new(AtomicBool::new(true)),
            guild_colors: Arc::new(RwLock::new(HashMap::new())),
            guild_colors_dirty: Arc::new(AtomicBool::new(true)),
            latest_scalar_sample: Arc::new(RwLock::new(None)),
            http_client,
            db,
            seq_live_handoff_v1: seq_live_handoff_v1_enabled(),
            observability: Arc::new(ObservabilityCounters::default()),
        }
    }
}
