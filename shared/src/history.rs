use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Ownership record for a single territory at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnershipRecord {
    pub guild_uuid: String,
    pub guild_name: String,
    pub guild_prefix: String,
    pub acquired_at: String,
}

/// Reconstructed state of all territory ownership at a specific timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySnapshot {
    pub timestamp: String,
    pub ownership: HashMap<String, OwnershipRecord>,
}

/// A single territory change event from the history log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    #[serde(default)]
    pub stream_seq: u64,
    pub timestamp: String,
    pub territory: String,
    pub guild_uuid: String,
    pub guild_name: String,
    pub guild_prefix: String,
    pub prev_guild_name: Option<String>,
    pub prev_guild_prefix: Option<String>,
}

/// Paginated list of history events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvents {
    pub events: Vec<HistoryEvent>,
    pub has_more: bool,
}

/// Time bounds and event count for the history timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryBounds {
    pub earliest: Option<String>,
    pub latest: Option<String>,
    pub event_count: i64,
    pub latest_seq: Option<u64>,
}
