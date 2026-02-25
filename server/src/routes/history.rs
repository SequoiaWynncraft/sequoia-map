use std::collections::HashMap;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use chrono::{DateTime, Utc};
use sequoia_shared::history::{
    HistoryBounds, HistoryEvent, HistoryEvents, HistorySnapshot, OwnershipRecord,
};
use serde::Deserialize;

use crate::state::AppState;

type HistoryEventRow = (
    i64,
    DateTime<Utc>,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);
type HistoryBoundsRow = (
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    i64,
    Option<i64>,
);

#[derive(Deserialize)]
pub struct AtQuery {
    t: String,
}

#[derive(Deserialize)]
pub struct EventsQuery {
    from: String,
    to: String,
    #[serde(default)]
    after_seq: Option<u64>,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    500
}

/// `GET /api/history/at?t={rfc3339}` — Reconstruct ownership at a point in time.
pub async fn history_at(
    State(state): State<AppState>,
    Query(query): Query<AtQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let pool = state.db.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let target: DateTime<Utc> = query
        .t
        .parse::<DateTime<Utc>>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // 1. Find nearest snapshot before target
    let snapshot_row: Option<(i64, DateTime<Utc>, serde_json::Value)> = sqlx::query_as(
        "SELECT id, created_at, ownership FROM territory_snapshots \
         WHERE created_at <= $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(target)
    .fetch_optional(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (mut ownership, snapshot_time) = match snapshot_row {
        Some((_id, created_at, ownership_json)) => {
            let ownership: HashMap<String, OwnershipRecord> =
                serde_json::from_value(ownership_json)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            (ownership, Some(created_at))
        }
        None => (HashMap::new(), None),
    };

    // 2. Replay events from snapshot time to target
    let events_from = snapshot_time.unwrap_or(DateTime::UNIX_EPOCH);

    let event_rows: Vec<(String, String, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT territory, guild_uuid, guild_name, guild_prefix, \
                acquired_at \
         FROM territory_events \
         WHERE recorded_at > $1 AND recorded_at <= $2 \
         ORDER BY stream_seq ASC",
    )
    .bind(events_from)
    .bind(target)
    .fetch_all(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    for (territory, guild_uuid, guild_name, guild_prefix, acquired_at) in event_rows {
        ownership.insert(
            territory,
            OwnershipRecord {
                guild_uuid,
                guild_name,
                guild_prefix,
                acquired_at: acquired_at.to_rfc3339(),
            },
        );
    }

    let snapshot = HistorySnapshot {
        timestamp: target.to_rfc3339(),
        ownership,
    };

    // Cache older timestamps aggressively, recent ones briefly
    let age_secs = (Utc::now() - target).num_seconds();
    let max_age = if age_secs > 3600 { 86400 } else { 60 };

    let mut headers = HeaderMap::new();
    let cache_control = HeaderValue::from_str(&format!("public, max-age={max_age}"))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    headers.insert(header::CACHE_CONTROL, cache_control);

    Ok((headers, Json(snapshot)))
}

/// `GET /api/history/events?from={t}&to={t}&limit={n}&after_seq={seq}` — Paginated event list.
pub async fn history_events(
    State(state): State<AppState>,
    Query(query): Query<EventsQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let pool = state.db.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let from: DateTime<Utc> = query
        .from
        .parse::<DateTime<Utc>>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let to: DateTime<Utc> = query
        .to
        .parse::<DateTime<Utc>>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let limit = query.limit.clamp(1, 1000);
    let after_seq = match query.after_seq {
        Some(seq) => Some(i64::try_from(seq).map_err(|_| StatusCode::BAD_REQUEST)?),
        None => None,
    };

    let rows: Vec<HistoryEventRow> = if let Some(after_seq) = after_seq {
        sqlx::query_as(
            "SELECT stream_seq, recorded_at, territory, guild_uuid, guild_name, guild_prefix, \
                    prev_guild_name, prev_guild_prefix \
             FROM territory_events \
             WHERE stream_seq > $1 AND recorded_at > $2 AND recorded_at <= $3 \
             ORDER BY stream_seq ASC \
             LIMIT $4",
        )
        .bind(after_seq)
        .bind(from)
        .bind(to)
        .bind(limit + 1)
        .fetch_all(pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        sqlx::query_as(
            "SELECT stream_seq, recorded_at, territory, guild_uuid, guild_name, guild_prefix, \
                    prev_guild_name, prev_guild_prefix \
             FROM territory_events \
             WHERE recorded_at > $1 AND recorded_at <= $2 \
             ORDER BY stream_seq ASC \
             LIMIT $3",
        )
        .bind(from)
        .bind(to)
        .bind(limit + 1)
        .fetch_all(pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let has_more = rows.len() as i64 > limit;
    let mut events = Vec::with_capacity(limit as usize);
    for (
        stream_seq,
        recorded_at,
        territory,
        guild_uuid,
        guild_name,
        guild_prefix,
        prev_guild_name,
        prev_guild_prefix,
    ) in rows.into_iter().take(limit as usize)
    {
        events.push(HistoryEvent {
            stream_seq: u64::try_from(stream_seq).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            timestamp: recorded_at.to_rfc3339(),
            territory,
            guild_uuid,
            guild_name,
            guild_prefix,
            prev_guild_name,
            prev_guild_prefix,
        });
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=60"),
    );

    Ok((headers, Json(HistoryEvents { events, has_more })))
}

/// `GET /api/history/bounds` — Returns earliest/latest timestamps and event count.
pub async fn history_bounds(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let pool = state.db.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let row: HistoryBoundsRow = sqlx::query_as(
        "SELECT MIN(recorded_at), MAX(recorded_at), COUNT(*), MAX(stream_seq) FROM territory_events",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let bounds = HistoryBounds {
        earliest: row.0.map(|dt| dt.to_rfc3339()),
        latest: row.1.map(|dt| dt.to_rfc3339()),
        event_count: row.2,
        latest_seq: row.3.and_then(|v| u64::try_from(v).ok()),
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=30"),
    );

    Ok((headers, Json(bounds)))
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use chrono::Utc;
    use reqwest::StatusCode;
    use sequoia_shared::history::HistoryEvents;
    use sqlx::postgres::PgPoolOptions;

    use crate::state::AppState;

    fn lazy_test_pool() -> sqlx::PgPool {
        PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://sequoia:sequoia@localhost/sequoia")
            .expect("lazy test pool should parse")
    }

    async fn spawn_test_server(state: AppState) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener address");
        let app = crate::app::build_app(state);
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });
        (addr, handle)
    }

    #[tokio::test]
    async fn invalid_history_query_params_return_bad_request() {
        let state = AppState::new(Some(lazy_test_pool()));
        let (addr, server_handle) = spawn_test_server(state).await;
        let base_url = format!("http://{addr}");
        let client = reqwest::Client::new();

        let at_status = client
            .get(format!("{base_url}/api/history/at?t=not-a-timestamp"))
            .send()
            .await
            .expect("history at request")
            .status();
        assert_eq!(at_status, StatusCode::BAD_REQUEST);

        let events_status = client
            .get(format!(
                "{base_url}/api/history/events?from=nope&to=also-nope"
            ))
            .send()
            .await
            .expect("history events request")
            .status();
        assert_eq!(events_status, StatusCode::BAD_REQUEST);

        server_handle.abort();
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn history_events_paginates_with_after_seq() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("Skipping real-Postgres history pagination test: DATABASE_URL is not set");
            return;
        };

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("connect real postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        sqlx::query("TRUNCATE TABLE territory_events, territory_snapshots RESTART IDENTITY")
            .execute(&pool)
            .await
            .expect("truncate history tables");

        let now = Utc::now();
        let acquired_1 = now - chrono::TimeDelta::minutes(1);
        let acquired_2 = now;
        sqlx::query(
            "INSERT INTO territory_events \
             (stream_seq, acquired_at, territory, guild_uuid, guild_name, guild_prefix, \
              prev_guild_uuid, prev_guild_name, prev_guild_prefix) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(1_i64)
        .bind(acquired_1)
        .bind("Alpha")
        .bind("g1")
        .bind("GuildOne")
        .bind("G1")
        .bind(None::<&str>)
        .bind(None::<&str>)
        .bind(None::<&str>)
        .execute(&pool)
        .await
        .expect("insert event 1");

        sqlx::query(
            "INSERT INTO territory_events \
             (stream_seq, acquired_at, territory, guild_uuid, guild_name, guild_prefix, \
              prev_guild_uuid, prev_guild_name, prev_guild_prefix) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(2_i64)
        .bind(acquired_2)
        .bind("Alpha")
        .bind("g2")
        .bind("GuildTwo")
        .bind("G2")
        .bind(Some("g1"))
        .bind(Some("GuildOne"))
        .bind(Some("G1"))
        .execute(&pool)
        .await
        .expect("insert event 2");

        let state = AppState::new(Some(pool));
        let (addr, server_handle) = spawn_test_server(state).await;
        let base_url = format!("http://{addr}");
        let client = reqwest::Client::new();

        let to = (Utc::now() + chrono::TimeDelta::minutes(1)).to_rfc3339();
        let all_url = {
            let mut url = reqwest::Url::parse(&format!("{base_url}/api/history/events"))
                .expect("history events url");
            url.query_pairs_mut()
                .append_pair("from", "1970-01-01T00:00:00Z")
                .append_pair("to", &to)
                .append_pair("limit", "100");
            url
        };
        let all_events = client
            .get(all_url)
            .send()
            .await
            .expect("all events request")
            .error_for_status()
            .expect("all events status")
            .json::<HistoryEvents>()
            .await
            .expect("parse all events");
        assert_eq!(all_events.events.len(), 2);
        assert_eq!(all_events.events[0].stream_seq, 1);
        assert_eq!(all_events.events[1].stream_seq, 2);

        let paged_url = {
            let mut url = reqwest::Url::parse(&format!("{base_url}/api/history/events"))
                .expect("paged history events url");
            url.query_pairs_mut()
                .append_pair("from", "1970-01-01T00:00:00Z")
                .append_pair("to", &to)
                .append_pair("after_seq", "1")
                .append_pair("limit", "100");
            url
        };
        let paged_events = client
            .get(paged_url)
            .send()
            .await
            .expect("paged events request")
            .error_for_status()
            .expect("paged events status")
            .json::<HistoryEvents>()
            .await
            .expect("parse paged events");
        assert_eq!(paged_events.events.len(), 1);
        assert_eq!(paged_events.events[0].stream_seq, 2);
        assert!(!paged_events.has_more);

        server_handle.abort();
        let _ = server_handle.await;
    }
}
