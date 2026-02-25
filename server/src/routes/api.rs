use std::fmt::Write as _;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use chrono::Utc;
use sequoia_shared::LiveState as LiveStatePayload;

use crate::config::{GUILD_CACHE_TTL_SECS, WYNNCRAFT_GUILD_URL};
use crate::state::{AppState, CachedGuild, ObservabilitySnapshot};

const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

pub async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let territory_count = state.live_snapshot.read().await.territories.len();
    let observability = state.observability.snapshot();
    Json(serde_json::json!({
        "status": "ok",
        "territories": territory_count,
        "guild_cache_size": state.guild_cache.len(),
        "history_available": state.db.is_some(),
        "seq_live_handoff_v1": state.seq_live_handoff_v1,
        "observability": {
            "live_state_requests_total": observability.live_state_requests_total,
            "persist_failures_total": observability.persist_failures_total,
            "dropped_update_events_total": observability.dropped_update_events_total,
            "persisted_update_events_total": observability.persisted_update_events_total,
        }
    }))
}

/// Serve pre-serialized TerritoryMap JSON — no HashMap clone, no re-serialization.
pub async fn get_territories(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (etag, json): (String, Arc<String>) = {
        let snapshot = state.live_snapshot.read().await;
        (
            territories_etag(snapshot.seq),
            Arc::clone(&snapshot.territories_json),
        )
    };

    if if_none_match_matches(&headers, &etag) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        let response_headers = response.headers_mut();
        response_headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=5"),
        );
        if let Ok(etag_header) = HeaderValue::from_str(&etag) {
            response_headers.insert(header::ETAG, etag_header);
        }
        return response;
    }

    let mut response = (StatusCode::OK, Arc::unwrap_or_clone(json)).into_response();
    let response_headers = response.headers_mut();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response_headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=5"),
    );
    if let Ok(etag_header) = HeaderValue::from_str(&etag) {
        response_headers.insert(header::ETAG, etag_header);
    }
    response
}

pub async fn get_live_state(State(state): State<AppState>) -> Json<LiveStatePayload> {
    state.observability.record_live_state_request();
    let snapshot = state.live_snapshot.read().await;
    Json(LiveStatePayload {
        seq: snapshot.seq,
        timestamp: snapshot.timestamp.clone(),
        territories: snapshot.territories.clone(),
    })
}

pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let territory_count = state.live_snapshot.read().await.territories.len();
    let guild_cache_size = state.guild_cache.len();
    let history_available = state.db.is_some();
    let seq_live_handoff_v1 = state.seq_live_handoff_v1;
    let observability = state.observability.snapshot();

    let body = render_prometheus_metrics(
        territory_count,
        guild_cache_size,
        history_available,
        seq_live_handoff_v1,
        observability,
    );

    (
        [
            (header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
}

fn render_prometheus_metrics(
    territory_count: usize,
    guild_cache_size: usize,
    history_available: bool,
    seq_live_handoff_v1: bool,
    observability: ObservabilitySnapshot,
) -> String {
    let mut body = String::new();
    let _ = writeln!(
        body,
        "# HELP sequoia_territories Current number of territories in the live snapshot."
    );
    let _ = writeln!(body, "# TYPE sequoia_territories gauge");
    let _ = writeln!(body, "sequoia_territories {territory_count}");

    let _ = writeln!(
        body,
        "# HELP sequoia_guild_cache_size Current number of guild entries in cache."
    );
    let _ = writeln!(body, "# TYPE sequoia_guild_cache_size gauge");
    let _ = writeln!(body, "sequoia_guild_cache_size {guild_cache_size}");

    let _ = writeln!(
        body,
        "# HELP sequoia_history_available Whether history storage is available (1 or 0)."
    );
    let _ = writeln!(body, "# TYPE sequoia_history_available gauge");
    let _ = writeln!(
        body,
        "sequoia_history_available {}",
        u8::from(history_available)
    );

    let _ = writeln!(
        body,
        "# HELP sequoia_seq_live_handoff_v1_enabled Whether seq live handoff v1 is enabled (1 or 0)."
    );
    let _ = writeln!(body, "# TYPE sequoia_seq_live_handoff_v1_enabled gauge");
    let _ = writeln!(
        body,
        "sequoia_seq_live_handoff_v1_enabled {}",
        u8::from(seq_live_handoff_v1)
    );

    let _ = writeln!(
        body,
        "# HELP sequoia_live_state_requests_total Total live-state API requests."
    );
    let _ = writeln!(body, "# TYPE sequoia_live_state_requests_total counter");
    let _ = writeln!(
        body,
        "sequoia_live_state_requests_total {}",
        observability.live_state_requests_total
    );

    let _ = writeln!(
        body,
        "# HELP sequoia_persist_failures_total Total failures while persisting update events."
    );
    let _ = writeln!(body, "# TYPE sequoia_persist_failures_total counter");
    let _ = writeln!(
        body,
        "sequoia_persist_failures_total {}",
        observability.persist_failures_total
    );

    let _ = writeln!(
        body,
        "# HELP sequoia_dropped_update_events_total Total update events dropped before broadcast."
    );
    let _ = writeln!(body, "# TYPE sequoia_dropped_update_events_total counter");
    let _ = writeln!(
        body,
        "sequoia_dropped_update_events_total {}",
        observability.dropped_update_events_total
    );

    let _ = writeln!(
        body,
        "# HELP sequoia_persisted_update_events_total Total update events persisted before broadcast."
    );
    let _ = writeln!(body, "# TYPE sequoia_persisted_update_events_total counter");
    let _ = writeln!(
        body,
        "sequoia_persisted_update_events_total {}",
        observability.persisted_update_events_total
    );

    body
}

pub async fn get_guild(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    // Check cache
    if let Some(cached) = state.guild_cache.get(&name) {
        let age = Utc::now()
            .signed_duration_since(cached.cached_at)
            .num_seconds();
        if age < GUILD_CACHE_TTL_SECS {
            return Ok((
                [(header::CACHE_CONTROL, "public, max-age=300")],
                Json(cached.data.clone()),
            ));
        }
    }

    // Fetch from Wynncraft API
    let url = format!("{}/{}", WYNNCRAFT_GUILD_URL, name);
    let resp = state
        .http_client
        .get(&url)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    if !resp.status().is_success() {
        return Err(StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY));
    }

    let data: serde_json::Value = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

    // Cache it
    state.guild_cache.insert(
        name,
        CachedGuild {
            data: data.clone(),
            cached_at: Utc::now(),
        },
    );

    Ok(([(header::CACHE_CONTROL, "public, max-age=300")], Json(data)))
}

fn territories_etag(seq: u64) -> String {
    format!("\"territories-{seq}\"")
}

fn normalize_etag(candidate: &str) -> &str {
    candidate.strip_prefix("W/").unwrap_or(candidate).trim()
}

fn if_none_match_matches(headers: &HeaderMap, etag: &str) -> bool {
    let Some(value) = headers.get(header::IF_NONE_MATCH) else {
        return false;
    };
    let Ok(raw) = value.to_str() else {
        return false;
    };

    raw.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || normalize_etag(candidate) == normalize_etag(etag)
    })
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::{if_none_match_matches, render_prometheus_metrics};
    use crate::state::{AppState, ObservabilitySnapshot};

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

    #[test]
    fn metrics_output_contains_prometheus_help_type_and_values() {
        let observability = ObservabilitySnapshot {
            live_state_requests_total: 12,
            persist_failures_total: 3,
            dropped_update_events_total: 7,
            persisted_update_events_total: 99,
        };

        let metrics = render_prometheus_metrics(42, 5, true, false, observability);

        assert!(metrics.contains("# HELP sequoia_territories"));
        assert!(metrics.contains("# TYPE sequoia_live_state_requests_total counter"));
        assert!(metrics.contains("sequoia_territories 42"));
        assert!(metrics.contains("sequoia_guild_cache_size 5"));
        assert!(metrics.contains("sequoia_history_available 1"));
        assert!(metrics.contains("sequoia_seq_live_handoff_v1_enabled 0"));
        assert!(metrics.contains("sequoia_live_state_requests_total 12"));
        assert!(metrics.contains("sequoia_persist_failures_total 3"));
        assert!(metrics.contains("sequoia_dropped_update_events_total 7"));
        assert!(metrics.contains("sequoia_persisted_update_events_total 99"));
    }

    #[tokio::test]
    async fn health_and_metrics_expose_expected_contract() {
        let state = AppState::new(None);
        let (addr, server_handle) = spawn_test_server(state).await;
        let base_url = format!("http://{addr}");
        let client = reqwest::Client::new();

        client
            .get(format!("{base_url}/api/live/state"))
            .send()
            .await
            .expect("live-state request")
            .error_for_status()
            .expect("live-state status");

        let health = client
            .get(format!("{base_url}/api/health"))
            .send()
            .await
            .expect("health request")
            .error_for_status()
            .expect("health status")
            .json::<serde_json::Value>()
            .await
            .expect("parse health");

        assert_eq!(health.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert_eq!(
            health.get("history_available").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert!(
            health
                .get("observability")
                .and_then(|v| v.get("live_state_requests_total"))
                .and_then(|v| v.as_u64())
                .is_some()
        );

        let metrics = client
            .get(format!("{base_url}/api/metrics"))
            .send()
            .await
            .expect("metrics request")
            .error_for_status()
            .expect("metrics status")
            .text()
            .await
            .expect("parse metrics text");

        assert!(metrics.contains("# TYPE sequoia_live_state_requests_total counter"));
        assert!(metrics.contains("# TYPE sequoia_history_available gauge"));
        assert!(metrics.contains("sequoia_live_state_requests_total 1"));
        assert!(metrics.contains("sequoia_history_available 0"));

        server_handle.abort();
        let _ = server_handle.await;
    }

    #[test]
    fn if_none_match_supports_weak_and_multiple_etags() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::IF_NONE_MATCH,
            axum::http::HeaderValue::from_static("W/\"other\", \"territories-42\""),
        );
        assert!(if_none_match_matches(&headers, "\"territories-42\""));
    }

    #[tokio::test]
    async fn territories_endpoint_returns_not_modified_when_etag_matches() {
        let state = AppState::new(None);
        {
            let mut snapshot = state.live_snapshot.write().await;
            snapshot.seq = 9;
            snapshot.territories_json = std::sync::Arc::new("{\"Alpha\":{}}".to_string());
        }

        let (addr, server_handle) = spawn_test_server(state).await;
        let base_url = format!("http://{addr}");
        let client = reqwest::Client::new();

        let first = client
            .get(format!("{base_url}/api/territories"))
            .send()
            .await
            .expect("territories request should succeed");
        let first_status = first.status();
        let first_etag = first
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .expect("etag header should be present");
        let first_body = first.text().await.expect("read first response body");

        assert_eq!(first_status, reqwest::StatusCode::OK);
        assert_eq!(first_body, "{\"Alpha\":{}}");

        let second = client
            .get(format!("{base_url}/api/territories"))
            .header(reqwest::header::IF_NONE_MATCH, first_etag)
            .send()
            .await
            .expect("conditional territories request should succeed");

        assert_eq!(second.status(), reqwest::StatusCode::NOT_MODIFIED);
        assert_eq!(
            second
                .headers()
                .get(reqwest::header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("public, max-age=5")
        );

        server_handle.abort();
        let _ = server_handle.await;
    }
}
