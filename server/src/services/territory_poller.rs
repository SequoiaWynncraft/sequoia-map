use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::Utc;
use sequoia_shared::{GuildRef, TerritoryChange, TerritoryMap};
use tracing::{info, warn};

use crate::config::{POLL_INTERVAL_SECS, WYNNCRAFT_TERRITORY_URL};
use crate::state::{AppState, PreSerializedEvent};

type SequencedUpdates = Vec<(u64, TerritoryChange)>;
type PersistResultFuture<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

pub async fn run(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));

    loop {
        interval.tick().await;

        match fetch_territories(&state.http_client).await {
            Ok(mut new_map) => {
                // 0. Merge extra territory data (resources + connections)
                {
                    let extra = state.extra_terr.read().await;
                    for (name, terr) in new_map.iter_mut() {
                        if let Some(info) = extra.get(name) {
                            terr.resources = info.resources.clone();
                            terr.connections = info.connections.clone();
                        }
                    }
                }

                // 0b. Merge guild colors from Athena
                {
                    let colors = state.guild_colors.read().await;
                    for terr in new_map.values_mut() {
                        if let Some(&rgb) = colors.get(&terr.guild.name) {
                            terr.guild.color = Some(rgb);
                        }
                    }
                }

                process_polled_map(&state, new_map).await;
            }
            Err(e) => {
                warn!("Failed to fetch territories: {e}");
            }
        }
    }
}

async fn process_polled_map(state: &AppState, new_map: TerritoryMap) {
    process_polled_map_with(state, new_map, |pool, updates| {
        Box::pin(persist_updates(pool, updates))
    })
    .await;
}

async fn process_polled_map_with<F>(state: &AppState, new_map: TerritoryMap, persist_updates_fn: F)
where
    F: for<'a> FnOnce(&'a sqlx::PgPool, SequencedUpdates) -> PersistResultFuture<'a>,
{
    // 1. Read lock: compute diff, then release
    let (changes, has_removals, mut live_seq, mut live_timestamp) = {
        let current = state.live_snapshot.read().await;
        (
            compute_diff(&current.territories, &new_map),
            has_removed_territories(&current.territories, &new_map),
            current.seq,
            current.timestamp.clone(),
        )
    };

    let initial_seq = state.next_seq.load(Ordering::Relaxed);
    let mut seq_cursor = initial_seq;
    let mut outgoing = Vec::new();
    let mut sequenced_updates: SequencedUpdates = Vec::new();
    let mut snapshot_event_json: Option<Arc<String>> = None;

    if has_removals {
        let Some(seq) = seq_cursor.checked_add(1) else {
            warn!("Sequence counter overflow while preparing snapshot event");
            return;
        };
        seq_cursor = seq;
        let timestamp = Utc::now().to_rfc3339();
        let snapshot_json =
            match serialize_snapshot_event(seq, &new_map, &timestamp, "snapshot broadcast event") {
                Some(json) => json,
                None => return,
            };
        info!("territory set changed (removals detected), broadcasting snapshot");
        live_seq = seq;
        live_timestamp = timestamp;
        snapshot_event_json = Some(Arc::clone(&snapshot_json));
        outgoing.push(PreSerializedEvent::Snapshot {
            seq,
            json: snapshot_json,
        });
    } else if !changes.is_empty() {
        let timestamp = Utc::now().to_rfc3339();
        info!("{} territory changes detected", changes.len());
        let mut update_build_failed = false;

        for change in changes {
            let Some(seq) = seq_cursor.checked_add(1) else {
                warn!("Sequence counter overflow while preparing update event");
                update_build_failed = true;
                break;
            };
            seq_cursor = seq;
            let update_json = match serialize_update_event(
                seq,
                std::slice::from_ref(&change),
                &timestamp,
                "update broadcast event",
            ) {
                Some(json) => json,
                None => {
                    update_build_failed = true;
                    break;
                }
            };
            live_seq = seq;
            live_timestamp = timestamp.clone();
            outgoing.push(PreSerializedEvent::Update {
                seq,
                json: update_json,
            });
            sequenced_updates.push((seq, change));
        }

        if update_build_failed {
            return;
        }
    }

    if !sequenced_updates.is_empty() {
        let sequenced_update_count = sequenced_updates.len() as u64;
        match state.db.as_ref() {
            Some(pool) => {
                if let Err(e) = persist_updates_fn(pool, sequenced_updates).await {
                    state.observability.record_persist_failure();
                    state
                        .observability
                        .record_dropped_update_events(sequenced_update_count);
                    warn!(
                        dropped_update_events = sequenced_update_count,
                        error = %e,
                        "failed to persist updates; continuing with in-memory live update"
                    );
                } else {
                    state
                        .observability
                        .record_persisted_update_events(sequenced_update_count);
                }
            }
            None => {
                state
                    .observability
                    .record_dropped_update_events(sequenced_update_count);
                warn!(
                    dropped_update_events = sequenced_update_count,
                    "database unavailable; continuing with in-memory live update only"
                );
            }
        }
    }

    let snapshot_json = match snapshot_event_json {
        Some(json) => json,
        None => match serialize_snapshot_event(
            live_seq,
            &new_map,
            &live_timestamp,
            "live snapshot cache payload",
        ) {
            Some(json) => json,
            None => return,
        },
    };
    let territories_json = match serialize_territories(&new_map) {
        Some(json) => json,
        None => return,
    };

    {
        let mut current = state.live_snapshot.write().await;
        current.territories = new_map;
        current.snapshot_json = Arc::clone(&snapshot_json);
        current.territories_json = territories_json;
        current.seq = live_seq;
        current.timestamp = live_timestamp;
    }

    if seq_cursor != initial_seq {
        state.next_seq.store(seq_cursor, Ordering::Relaxed);
    }

    for event in outgoing {
        let _ = state.event_tx.send(event);
    }
}

#[derive(serde::Serialize)]
#[serde(tag = "type")]
enum SerializedTerritoryEvent<'a> {
    Snapshot {
        seq: u64,
        territories: &'a TerritoryMap,
        timestamp: &'a str,
    },
    Update {
        seq: u64,
        changes: &'a [TerritoryChange],
        timestamp: &'a str,
    },
}

fn serialize_snapshot_event(
    seq: u64,
    territories: &TerritoryMap,
    timestamp: &str,
    context: &str,
) -> Option<Arc<String>> {
    match serde_json::to_string(&SerializedTerritoryEvent::Snapshot {
        seq,
        territories,
        timestamp,
    }) {
        Ok(json) => Some(Arc::new(json)),
        Err(e) => {
            warn!("failed to serialize {context}: {e}");
            None
        }
    }
}

fn serialize_update_event(
    seq: u64,
    changes: &[TerritoryChange],
    timestamp: &str,
    context: &str,
) -> Option<Arc<String>> {
    match serde_json::to_string(&SerializedTerritoryEvent::Update {
        seq,
        changes,
        timestamp,
    }) {
        Ok(json) => Some(Arc::new(json)),
        Err(e) => {
            warn!("failed to serialize {context}: {e}");
            None
        }
    }
}

fn serialize_territories(map: &TerritoryMap) -> Option<Arc<String>> {
    match serde_json::to_string(map) {
        Ok(json) => Some(Arc::new(json)),
        Err(e) => {
            warn!("failed to serialize live territory map: {e}");
            None
        }
    }
}

async fn persist_updates(
    pool: &sqlx::PgPool,
    sequenced_updates: SequencedUpdates,
) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin transaction: {e}"))?;

    for (seq, change) in sequenced_updates {
        let stream_seq =
            i64::try_from(seq).map_err(|_| format!("sequence {seq} is out of i64 range"))?;

        let acquired_at = chrono::DateTime::parse_from_rfc3339(&change.acquired)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| {
                warn!(
                    "invalid acquired timestamp for territory {} at seq {}; using now()",
                    change.territory, seq
                );
                chrono::Utc::now()
            });

        let (prev_uuid, prev_name, prev_prefix) = match &change.previous_guild {
            Some(g) => (
                Some(g.uuid.as_str()),
                Some(g.name.as_str()),
                Some(g.prefix.as_str()),
            ),
            None => (None, None, None),
        };

        sqlx::query(
            "INSERT INTO territory_events \
             (stream_seq, acquired_at, territory, guild_uuid, guild_name, guild_prefix, \
              prev_guild_uuid, prev_guild_name, prev_guild_prefix) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(stream_seq)
        .bind(acquired_at)
        .bind(&change.territory)
        .bind(&change.guild.uuid)
        .bind(&change.guild.name)
        .bind(&change.guild.prefix)
        .bind(prev_uuid)
        .bind(prev_name)
        .bind(prev_prefix)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            format!(
                "insert update seq {seq} for territory {}: {e}",
                change.territory
            )
        })?;
    }

    tx.commit()
        .await
        .map_err(|e| format!("commit transaction: {e}"))?;
    Ok(())
}

async fn fetch_territories(client: &reqwest::Client) -> Result<TerritoryMap, reqwest::Error> {
    let resp = client.get(WYNNCRAFT_TERRITORY_URL).send().await?;
    let map: TerritoryMap = resp.json().await?;
    Ok(map)
}

fn compute_diff(old: &TerritoryMap, new: &TerritoryMap) -> Vec<TerritoryChange> {
    let mut changes = Vec::new();

    for (name, new_territory) in new {
        let changed = match old.get(name) {
            Some(old_territory) => old_territory.guild.uuid != new_territory.guild.uuid,
            None => true, // new territory
        };

        if changed {
            let previous_guild = old.get(name).map(|t| GuildRef {
                uuid: t.guild.uuid.clone(),
                name: t.guild.name.clone(),
                prefix: t.guild.prefix.clone(),
                color: t.guild.color,
            });

            changes.push(TerritoryChange {
                territory: name.clone(),
                guild: GuildRef {
                    uuid: new_territory.guild.uuid.clone(),
                    name: new_territory.guild.name.clone(),
                    prefix: new_territory.guild.prefix.clone(),
                    color: new_territory.guild.color,
                },
                previous_guild,
                acquired: new_territory.acquired.to_rfc3339(),
                location: new_territory.location.clone(),
                resources: new_territory.resources.clone(),
                connections: new_territory.connections.clone(),
            });
        }
    }

    changes
}

fn has_removed_territories(old: &TerritoryMap, new: &TerritoryMap) -> bool {
    old.keys().any(|name| !new.contains_key(name))
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

    use super::{compute_diff, has_removed_territories, process_polled_map_with};
    use axum::Router;
    use chrono::Utc;
    use sequoia_shared::history::{HistoryBounds, HistoryEvents, HistorySnapshot};
    use sequoia_shared::{GuildRef, Region, Territory, TerritoryMap};
    use sqlx::postgres::PgPoolOptions;
    use tokio::sync::oneshot;

    use crate::state::{AppState, PreSerializedEvent};

    fn territory(guild_uuid: &str, guild_name: &str, guild_prefix: &str) -> Territory {
        Territory {
            guild: GuildRef {
                uuid: guild_uuid.to_string(),
                name: guild_name.to_string(),
                prefix: guild_prefix.to_string(),
                color: None,
            },
            acquired: Utc::now(),
            location: Region {
                start: [0, 0],
                end: [10, 10],
            },
            resources: Default::default(),
            connections: Vec::new(),
        }
    }

    fn single_territory_map(
        guild_uuid: &str,
        guild_name: &str,
        guild_prefix: &str,
    ) -> TerritoryMap {
        let mut map = TerritoryMap::new();
        map.insert(
            "Alpha".to_string(),
            territory(guild_uuid, guild_name, guild_prefix),
        );
        map
    }

    fn lazy_test_pool() -> sqlx::PgPool {
        PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://sequoia:sequoia@localhost/sequoia")
            .expect("lazy test pool should parse")
    }

    fn history_test_app(state: AppState) -> Router {
        crate::app::build_app(state)
    }

    async fn spawn_test_server(app: Router) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener address");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });
        (addr, handle)
    }

    #[test]
    fn compute_diff_reports_new_and_changed_territories() {
        let mut old = TerritoryMap::new();
        old.insert("Alpha".to_string(), territory("g1", "GuildOne", "G1"));

        let mut new = TerritoryMap::new();
        new.insert("Alpha".to_string(), territory("g2", "GuildTwo", "G2"));
        new.insert("Beta".to_string(), territory("g3", "GuildThree", "G3"));

        let mut diff = compute_diff(&old, &new);
        diff.sort_by(|a, b| a.territory.cmp(&b.territory));

        assert_eq!(diff.len(), 2);
        assert_eq!(diff[0].territory, "Alpha");
        assert_eq!(diff[0].guild.uuid, "g2");
        assert_eq!(
            diff[0]
                .previous_guild
                .as_ref()
                .map(|guild| guild.uuid.as_str()),
            Some("g1")
        );
        assert_eq!(diff[1].territory, "Beta");
        assert!(diff[1].previous_guild.is_none());
    }

    #[test]
    fn compute_diff_skips_unchanged_owners() {
        let mut old = TerritoryMap::new();
        old.insert("Alpha".to_string(), territory("g1", "GuildOne", "G1"));

        let mut new = TerritoryMap::new();
        new.insert("Alpha".to_string(), territory("g1", "GuildOne", "G1"));

        let diff = compute_diff(&old, &new);
        assert!(diff.is_empty());
    }

    #[test]
    fn removed_territories_detection_is_correct() {
        let mut old = TerritoryMap::new();
        old.insert("Alpha".to_string(), territory("g1", "GuildOne", "G1"));
        old.insert("Beta".to_string(), territory("g2", "GuildTwo", "G2"));

        let mut new = TerritoryMap::new();
        new.insert("Alpha".to_string(), territory("g1", "GuildOne", "G1"));

        assert!(has_removed_territories(&old, &new));

        new.insert("Beta".to_string(), territory("g2", "GuildTwo", "G2"));
        assert!(!has_removed_territories(&old, &new));
    }

    #[tokio::test]
    async fn waits_for_persist_before_advancing_live_and_broadcasting() {
        let state = AppState::new(Some(lazy_test_pool()));

        {
            let mut current = state.live_snapshot.write().await;
            current.territories = single_territory_map("g1", "GuildOne", "G1");
            current.seq = 7;
            current.timestamp = "2026-01-01T00:00:00Z".to_string();
        }
        state
            .next_seq
            .store(7, std::sync::atomic::Ordering::Relaxed);

        let mut rx = state.event_tx.subscribe();
        let new_map = single_territory_map("g2", "GuildTwo", "G2");
        let (release_tx, release_rx) = oneshot::channel::<()>();
        let persist_started = Arc::new(AtomicBool::new(false));
        let persist_started_for_task = Arc::clone(&persist_started);

        let worker = tokio::spawn({
            let state = state.clone();
            async move {
                process_polled_map_with(&state, new_map, move |_pool, updates| {
                    Box::pin(async move {
                        assert_eq!(updates.len(), 1);
                        assert_eq!(updates[0].0, 8);
                        persist_started_for_task.store(true, AtomicOrdering::Relaxed);
                        let _ = release_rx.await;
                        Ok(())
                    })
                })
                .await;
            }
        });

        while !persist_started.load(AtomicOrdering::Relaxed) {
            tokio::task::yield_now().await;
        }

        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
        {
            let current = state.live_snapshot.read().await;
            assert_eq!(current.seq, 7);
            assert_eq!(
                current
                    .territories
                    .get("Alpha")
                    .expect("territory should remain")
                    .guild
                    .uuid,
                "g1"
            );
        }
        assert_eq!(state.next_seq.load(std::sync::atomic::Ordering::Relaxed), 7);

        let _ = release_tx.send(());
        worker.await.expect("worker should complete");

        match rx.try_recv() {
            Ok(PreSerializedEvent::Update { seq, .. }) => assert_eq!(seq, 8),
            Ok(other) => panic!("expected update event, got {other:?}"),
            Err(e) => panic!("expected update event, got recv error: {e}"),
        }
        {
            let current = state.live_snapshot.read().await;
            assert_eq!(current.seq, 8);
            assert_eq!(
                current
                    .territories
                    .get("Alpha")
                    .expect("territory should exist")
                    .guild
                    .uuid,
                "g2"
            );
        }
        assert_eq!(state.next_seq.load(std::sync::atomic::Ordering::Relaxed), 8);
        let observability = state.observability.snapshot();
        assert_eq!(observability.persisted_update_events_total, 1);
        assert_eq!(observability.dropped_update_events_total, 0);
        assert_eq!(observability.persist_failures_total, 0);
    }

    #[tokio::test]
    async fn continues_live_update_when_database_is_unavailable() {
        let state = AppState::new(None);

        {
            let mut current = state.live_snapshot.write().await;
            current.territories = single_territory_map("g1", "GuildOne", "G1");
            current.seq = 11;
            current.timestamp = "2026-01-01T00:00:00Z".to_string();
        }
        state
            .next_seq
            .store(11, std::sync::atomic::Ordering::Relaxed);

        let mut rx = state.event_tx.subscribe();
        let new_map = single_territory_map("g2", "GuildTwo", "G2");

        process_polled_map_with(&state, new_map, |_pool, _updates| {
            Box::pin(async { Ok(()) })
        })
        .await;

        match rx.try_recv() {
            Ok(PreSerializedEvent::Update { seq, .. }) => assert_eq!(seq, 12),
            Ok(other) => panic!("expected update event, got {other:?}"),
            Err(e) => panic!("expected update event, got recv error: {e}"),
        }
        {
            let current = state.live_snapshot.read().await;
            assert_eq!(current.seq, 12);
            assert_eq!(
                current
                    .territories
                    .get("Alpha")
                    .expect("territory should update")
                    .guild
                    .uuid,
                "g2"
            );
        }
        assert_eq!(
            state.next_seq.load(std::sync::atomic::Ordering::Relaxed),
            12
        );
        let observability = state.observability.snapshot();
        assert_eq!(observability.persisted_update_events_total, 0);
        assert_eq!(observability.dropped_update_events_total, 1);
        assert_eq!(observability.persist_failures_total, 0);
    }

    #[tokio::test]
    async fn continues_live_update_when_persist_fails() {
        let state = AppState::new(Some(lazy_test_pool()));

        {
            let mut current = state.live_snapshot.write().await;
            current.territories = single_territory_map("g1", "GuildOne", "G1");
            current.seq = 21;
            current.timestamp = "2026-01-01T00:00:00Z".to_string();
        }
        state
            .next_seq
            .store(21, std::sync::atomic::Ordering::Relaxed);

        let mut rx = state.event_tx.subscribe();
        let new_map = single_territory_map("g2", "GuildTwo", "G2");

        process_polled_map_with(&state, new_map, |_pool, _updates| {
            Box::pin(async { Err("forced persist error".to_string()) })
        })
        .await;

        match rx.try_recv() {
            Ok(PreSerializedEvent::Update { seq, .. }) => assert_eq!(seq, 22),
            Ok(other) => panic!("expected update event, got {other:?}"),
            Err(e) => panic!("expected update event, got recv error: {e}"),
        }
        {
            let current = state.live_snapshot.read().await;
            assert_eq!(current.seq, 22);
            assert_eq!(
                current
                    .territories
                    .get("Alpha")
                    .expect("territory should update")
                    .guild
                    .uuid,
                "g2"
            );
        }
        assert_eq!(
            state.next_seq.load(std::sync::atomic::Ordering::Relaxed),
            22
        );
        let observability = state.observability.snapshot();
        assert_eq!(observability.persisted_update_events_total, 0);
        assert_eq!(observability.dropped_update_events_total, 1);
        assert_eq!(observability.persist_failures_total, 1);
    }

    #[tokio::test]
    async fn persists_updates_and_serves_history_endpoints_with_real_postgres() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("Skipping real-Postgres integration test: DATABASE_URL is not set");
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

        let state = AppState::new(Some(pool.clone()));
        {
            let mut current = state.live_snapshot.write().await;
            current.territories = single_territory_map("g1", "GuildOne", "G1");
            current.seq = 0;
            current.timestamp = "2026-01-01T00:00:00Z".to_string();
        }
        state
            .next_seq
            .store(0, std::sync::atomic::Ordering::Relaxed);

        process_polled_map_with(
            &state,
            single_territory_map("g2", "GuildTwo", "G2"),
            |pool, updates| Box::pin(async move { super::persist_updates(pool, updates).await }),
        )
        .await;

        let db_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM territory_events")
            .fetch_one(&pool)
            .await
            .expect("count history rows");
        assert_eq!(db_count.0, 1);

        let app = history_test_app(state.clone());
        let (addr, server_handle) = spawn_test_server(app).await;
        let base_url = format!("http://{addr}");
        let client = reqwest::Client::new();

        let bounds = client
            .get(format!("{base_url}/api/history/bounds"))
            .send()
            .await
            .expect("history bounds request")
            .error_for_status()
            .expect("history bounds status")
            .json::<HistoryBounds>()
            .await
            .expect("parse history bounds");
        assert_eq!(bounds.event_count, 1);
        assert_eq!(bounds.latest_seq, Some(1));

        let to = (Utc::now() + chrono::TimeDelta::minutes(1)).to_rfc3339();
        let events_url = {
            let mut url = reqwest::Url::parse(&format!("{base_url}/api/history/events"))
                .expect("history events url");
            url.query_pairs_mut()
                .append_pair("from", "1970-01-01T00:00:00Z")
                .append_pair("to", &to)
                .append_pair("limit", "100");
            url
        };
        let events = client
            .get(events_url)
            .send()
            .await
            .expect("history events request")
            .error_for_status()
            .expect("history events status")
            .json::<HistoryEvents>()
            .await
            .expect("parse history events");
        assert_eq!(events.events.len(), 1);
        let event = &events.events[0];
        assert_eq!(event.stream_seq, 1);
        assert_eq!(event.territory, "Alpha");
        assert_eq!(event.guild_uuid, "g2");
        assert_eq!(event.prev_guild_name.as_deref(), Some("GuildOne"));

        let at_url = {
            let mut url =
                reqwest::Url::parse(&format!("{base_url}/api/history/at")).expect("history at url");
            url.query_pairs_mut().append_pair("t", &to);
            url
        };
        let snapshot = client
            .get(at_url)
            .send()
            .await
            .expect("history at request")
            .error_for_status()
            .expect("history at status")
            .json::<HistorySnapshot>()
            .await
            .expect("parse history snapshot");
        let alpha = snapshot
            .ownership
            .get("Alpha")
            .expect("Alpha should exist in history snapshot");
        assert_eq!(alpha.guild_uuid, "g2");
        assert_eq!(alpha.guild_name, "GuildTwo");

        server_handle.abort();
        let _ = server_handle.await;
    }
}
