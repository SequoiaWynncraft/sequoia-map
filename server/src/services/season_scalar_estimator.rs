use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::future::join_all;
use serde::Deserialize;
use sqlx::{Postgres, QueryBuilder};
use tracing::{info, warn};

use sequoia_shared::{
    SeasonScalarCurrent, SeasonScalarSample, infer_scalar_raw, infer_scalar_weighted,
};

use crate::config::WYNNCRAFT_GUILD_URL;
use crate::state::AppState;

const ESTIMATOR_INTERVAL_SECS: u64 = 300;
const SCALAR_CANDIDATE_GUILDS: usize = 8;
const LEADERBOARD_SAMPLE_GUILDS: usize = 50;
const MAX_REASONABLE_SCALAR: f64 = 20.0;
type LatestScalarSampleRow = (DateTime<Utc>, i32, f64, f64, f64, i32);

#[derive(Debug, Clone)]
struct CandidateGuild {
    guild_name: String,
    guild_uuid: String,
    guild_prefix: String,
    territory_count: usize,
}

#[derive(Debug, Clone)]
struct GuildObservation {
    observed_at: DateTime<Utc>,
    season_id: i32,
    rating: i64,
    territory_count: usize,
}

#[derive(Debug, Clone)]
struct GuildSeasonSnapshot {
    guild_name: String,
    guild_uuid: String,
    guild_prefix: String,
    observed_at: DateTime<Utc>,
    season_id: i32,
    rating: i64,
    territory_count: usize,
}

#[derive(Debug, Clone)]
struct ScalarEstimate {
    season_id: i32,
    scalar_weighted: f64,
    scalar_raw: f64,
}

#[derive(Debug, Deserialize)]
struct GuildSeasonRank {
    rating: i64,
}

#[derive(Debug, Deserialize)]
struct GuildPayload {
    #[serde(default, rename = "seasonRanks")]
    season_ranks: HashMap<String, GuildSeasonRank>,
}

pub async fn run(state: AppState) {
    let Some(pool) = state.db.as_ref().cloned() else {
        warn!("season scalar estimator disabled: no database configured");
        return;
    };

    info!(
        interval_secs = ESTIMATOR_INTERVAL_SECS,
        scalar_candidate_guilds = SCALAR_CANDIDATE_GUILDS,
        leaderboard_sample_guilds = LEADERBOARD_SAMPLE_GUILDS,
        "season scalar estimator started"
    );

    warm_cache(&state).await;

    let mut interval = tokio::time::interval(Duration::from_secs(ESTIMATOR_INTERVAL_SECS));
    let mut previous: HashMap<String, GuildObservation> = HashMap::new();

    loop {
        interval.tick().await;

        if let Err(e) = sample_once(&state, &pool, &mut previous).await {
            warn!(error = %e, "season scalar estimator tick failed");
        }
    }
}

pub async fn warm_cache(state: &AppState) {
    let Some(pool) = state.db.as_ref() else {
        return;
    };
    if let Err(e) = refresh_latest_scalar_cache(state, pool).await {
        warn!(error = %e, "failed to warm season scalar cache from database");
    }
}

async fn sample_once(
    state: &AppState,
    pool: &sqlx::PgPool,
    previous: &mut HashMap<String, GuildObservation>,
) -> Result<(), String> {
    let sampled_candidates = top_candidate_guilds(state, LEADERBOARD_SAMPLE_GUILDS).await;
    if sampled_candidates.is_empty() {
        return Ok(());
    }
    let scalar_candidates: HashSet<&str> = sampled_candidates
        .iter()
        .take(SCALAR_CANDIDATE_GUILDS)
        .map(|candidate| candidate.guild_name.as_str())
        .collect();

    let now = Utc::now();
    let futures = sampled_candidates
        .iter()
        .map(|candidate| fetch_guild_snapshot(&state.http_client, candidate, now));
    let snapshots: Vec<GuildSeasonSnapshot> =
        join_all(futures).await.into_iter().flatten().collect();
    if snapshots.is_empty() {
        return Ok(());
    }

    let mut estimates: Vec<ScalarEstimate> = Vec::new();
    for snapshot in &snapshots {
        if scalar_candidates.contains(snapshot.guild_name.as_str())
            && let Some(prev) = previous.get(&snapshot.guild_name)
            && let Some(estimate) = derive_scalar_estimate(prev, snapshot)
        {
            estimates.push(estimate);
        }

        previous.insert(
            snapshot.guild_name.clone(),
            GuildObservation {
                observed_at: snapshot.observed_at,
                season_id: snapshot.season_id,
                rating: snapshot.rating,
                territory_count: snapshot.territory_count,
            },
        );
    }

    let keep: HashSet<&str> = sampled_candidates
        .iter()
        .map(|candidate| candidate.guild_name.as_str())
        .collect();
    previous.retain(|guild_name, _| keep.contains(guild_name.as_str()));

    persist_guild_observations(pool, &snapshots).await?;

    if estimates.is_empty() {
        return Ok(());
    }

    let Some((season_id, season_estimates)) = select_dominant_season(estimates) else {
        return Ok(());
    };
    if season_estimates.is_empty() {
        return Ok(());
    }

    let mut weighted: Vec<f64> = season_estimates.iter().map(|e| e.scalar_weighted).collect();
    let mut raw: Vec<f64> = season_estimates.iter().map(|e| e.scalar_raw).collect();

    let scalar_weighted = median(&mut weighted).ok_or("missing weighted scalar median")?;
    let scalar_raw = median(&mut raw).ok_or("missing raw scalar median")?;

    let spread = iqr(&weighted);
    let confidence = compute_confidence(season_estimates.len(), scalar_weighted, spread);
    let sample_count = i32::try_from(season_estimates.len()).unwrap_or(i32::MAX);

    sqlx::query(
        "INSERT INTO season_scalar_samples \
         (sampled_at, season_id, scalar_weighted, scalar_raw, confidence, sample_count) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(now)
    .bind(season_id)
    .bind(scalar_weighted)
    .bind(scalar_raw)
    .bind(confidence)
    .bind(sample_count)
    .execute(pool)
    .await
    .map_err(|e| format!("insert season scalar sample: {e}"))?;

    let sample = SeasonScalarSample {
        sampled_at: now.to_rfc3339(),
        season_id,
        scalar_weighted,
        scalar_raw,
        confidence,
        sample_count: u32::try_from(sample_count.max(0)).unwrap_or(u32::MAX),
    };
    if let Some(cached_sample) = build_cached_scalar_sample(sample.clone()) {
        let mut latest = state.latest_scalar_sample.write().await;
        *latest = Some(cached_sample);
    }

    info!(
        season_id,
        sample_count,
        scalar_weighted = format_args!("{scalar_weighted:.4}"),
        scalar_raw = format_args!("{scalar_raw:.4}"),
        confidence = format_args!("{confidence:.4}"),
        "persisted season scalar sample"
    );

    Ok(())
}

async fn persist_guild_observations(
    pool: &sqlx::PgPool,
    snapshots: &[GuildSeasonSnapshot],
) -> Result<(), String> {
    if snapshots.is_empty() {
        return Ok(());
    }

    #[derive(Debug)]
    struct ObservationInsertRow {
        observed_at: DateTime<Utc>,
        season_id: i32,
        guild_name: String,
        guild_uuid: String,
        guild_prefix: String,
        territory_count: i16,
        season_rating: i32,
        sr_gain_5m: Option<i32>,
        sample_rank: i32,
    }

    let guild_names: Vec<String> = snapshots.iter().map(|row| row.guild_name.clone()).collect();
    let latest_rows: Vec<(String, i32, i32)> = sqlx::query_as(
        "SELECT DISTINCT ON (guild_name) guild_name, season_id, season_rating \
         FROM season_guild_observations \
         WHERE guild_name = ANY($1) \
         ORDER BY guild_name, observed_at DESC",
    )
    .bind(&guild_names)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load latest season observations: {e}"))?;
    let latest_by_guild: HashMap<String, (i32, i32)> = latest_rows
        .into_iter()
        .map(|(guild_name, season_id, season_rating)| (guild_name, (season_id, season_rating)))
        .collect();
    let sample_ranks = rank_snapshots(snapshots);

    let mut rows = Vec::with_capacity(snapshots.len());
    for snapshot in snapshots {
        let territory_count = i16::try_from(snapshot.territory_count).map_err(|_| {
            format!(
                "territory count {} is out of i16 range for guild {}",
                snapshot.territory_count, snapshot.guild_name
            )
        })?;
        let season_rating = i32::try_from(snapshot.rating).map_err(|_| {
            format!(
                "season rating {} is out of i32 range for guild {}",
                snapshot.rating, snapshot.guild_name
            )
        })?;
        let sr_gain_5m =
            latest_by_guild
                .get(&snapshot.guild_name)
                .and_then(|(season_id, previous_rating)| {
                    if *season_id != snapshot.season_id {
                        return None;
                    }
                    let delta = snapshot.rating - i64::from(*previous_rating);
                    i32::try_from(delta).ok()
                });
        let sample_rank = sample_ranks
            .get(&snapshot.guild_name)
            .copied()
            .ok_or_else(|| format!("missing sample rank for guild {}", snapshot.guild_name))?;

        rows.push(ObservationInsertRow {
            observed_at: snapshot.observed_at,
            season_id: snapshot.season_id,
            guild_name: snapshot.guild_name.clone(),
            guild_uuid: snapshot.guild_uuid.clone(),
            guild_prefix: snapshot.guild_prefix.clone(),
            territory_count,
            season_rating,
            sr_gain_5m,
            sample_rank,
        });
    }

    let mut query_builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO season_guild_observations \
         (observed_at, season_id, guild_name, guild_uuid, guild_prefix, territory_count, season_rating, sr_gain_5m, sample_rank) ",
    );
    query_builder.push_values(rows, |mut builder, row| {
        builder
            .push_bind(row.observed_at)
            .push_bind(row.season_id)
            .push_bind(row.guild_name)
            .push_bind(row.guild_uuid)
            .push_bind(row.guild_prefix)
            .push_bind(row.territory_count)
            .push_bind(row.season_rating)
            .push_bind(row.sr_gain_5m)
            .push_bind(row.sample_rank);
    });
    query_builder.push(" ON CONFLICT (observed_at, guild_name) DO NOTHING");
    query_builder
        .build()
        .execute(pool)
        .await
        .map_err(|e| format!("insert season guild observations: {e}"))?;

    Ok(())
}

fn rank_snapshots(snapshots: &[GuildSeasonSnapshot]) -> HashMap<String, i32> {
    let mut ranked: Vec<&GuildSeasonSnapshot> = snapshots.iter().collect();
    ranked.sort_by(|a, b| {
        b.rating
            .cmp(&a.rating)
            .then_with(|| b.territory_count.cmp(&a.territory_count))
            .then_with(|| a.guild_name.cmp(&b.guild_name))
    });

    ranked
        .into_iter()
        .enumerate()
        .map(|(idx, row)| {
            let rank = i32::try_from(idx + 1).unwrap_or(i32::MAX);
            (row.guild_name.clone(), rank)
        })
        .collect()
}

async fn refresh_latest_scalar_cache(state: &AppState, pool: &sqlx::PgPool) -> Result<(), String> {
    let row: Option<LatestScalarSampleRow> = sqlx::query_as(
        "SELECT sampled_at, season_id, scalar_weighted, scalar_raw, confidence, sample_count \
         FROM season_scalar_samples \
         ORDER BY sampled_at DESC \
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load latest season scalar sample: {e}"))?;

    let cached = row
        .map(
            |(sampled_at, season_id, scalar_weighted, scalar_raw, confidence, sample_count)| {
                SeasonScalarSample {
                    sampled_at: sampled_at.to_rfc3339(),
                    season_id,
                    scalar_weighted,
                    scalar_raw,
                    confidence,
                    sample_count: u32::try_from(sample_count.max(0)).unwrap_or(u32::MAX),
                }
            },
        )
        .and_then(build_cached_scalar_sample);

    let mut latest = state.latest_scalar_sample.write().await;
    *latest = cached;
    Ok(())
}

fn build_cached_scalar_sample(
    sample: SeasonScalarSample,
) -> Option<(SeasonScalarSample, Arc<Bytes>)> {
    match serde_json::to_vec(&SeasonScalarCurrent {
        sample: Some(sample.clone()),
    }) {
        Ok(json) => Some((sample, Arc::new(Bytes::from(json)))),
        Err(e) => {
            warn!(error = %e, "failed to serialize season scalar cache payload");
            None
        }
    }
}

async fn top_candidate_guilds(state: &AppState, limit: usize) -> Vec<CandidateGuild> {
    let snapshot = state.live_snapshot.read().await;
    if snapshot.territories.is_empty() {
        return Vec::new();
    }

    let mut candidates: HashMap<String, CandidateGuild> = HashMap::new();
    for territory in snapshot.territories.values() {
        let guild_name = territory.guild.name.clone();
        let entry = candidates
            .entry(guild_name.clone())
            .or_insert_with(|| CandidateGuild {
                guild_name,
                guild_uuid: territory.guild.uuid.clone(),
                guild_prefix: territory.guild.prefix.clone(),
                territory_count: 0,
            });
        entry.territory_count += 1;
    }
    drop(snapshot);

    let mut guilds: Vec<CandidateGuild> = candidates.into_values().collect();
    guilds.sort_by(|a, b| {
        b.territory_count
            .cmp(&a.territory_count)
            .then_with(|| a.guild_name.cmp(&b.guild_name))
    });
    guilds.truncate(limit);
    guilds
}

async fn fetch_guild_snapshot(
    client: &reqwest::Client,
    candidate: &CandidateGuild,
    observed_at: DateTime<Utc>,
) -> Option<GuildSeasonSnapshot> {
    let mut url = match reqwest::Url::parse(WYNNCRAFT_GUILD_URL) {
        Ok(url) => url,
        Err(e) => {
            warn!(error = %e, "invalid guild base URL");
            return None;
        }
    };
    {
        let Ok(mut segments) = url.path_segments_mut() else {
            warn!("failed to edit guild URL path segments");
            return None;
        };
        segments.push(candidate.guild_name.as_str());
    }

    let response = match client.get(url).send().await {
        Ok(resp) => resp,
        Err(e) => {
            warn!(guild = candidate.guild_name, error = %e, "guild fetch failed");
            return None;
        }
    };

    if !response.status().is_success() {
        warn!(
            guild = candidate.guild_name,
            status = response.status().as_u16(),
            "guild fetch returned non-success status"
        );
        return None;
    }

    let payload = match response.json::<GuildPayload>().await {
        Ok(payload) => payload,
        Err(e) => {
            warn!(guild = candidate.guild_name, error = %e, "guild response parse failed");
            return None;
        }
    };

    let (season_id, rating) = latest_season_rating(&payload.season_ranks)?;
    Some(GuildSeasonSnapshot {
        guild_name: candidate.guild_name.clone(),
        guild_uuid: candidate.guild_uuid.clone(),
        guild_prefix: candidate.guild_prefix.clone(),
        observed_at,
        season_id,
        rating,
        territory_count: candidate.territory_count,
    })
}

fn latest_season_rating(season_ranks: &HashMap<String, GuildSeasonRank>) -> Option<(i32, i64)> {
    season_ranks
        .iter()
        .filter_map(|(season, value)| season.parse::<i32>().ok().map(|id| (id, value.rating)))
        .max_by_key(|(season_id, _)| *season_id)
}

fn derive_scalar_estimate(
    previous: &GuildObservation,
    current: &GuildSeasonSnapshot,
) -> Option<ScalarEstimate> {
    if previous.season_id != current.season_id {
        return None;
    }
    if previous.territory_count != current.territory_count {
        return None;
    }

    let delta_secs = (current.observed_at - previous.observed_at).num_seconds() as f64;
    if delta_secs <= 0.0 {
        return None;
    }
    let delta_rating = current.rating as f64 - previous.rating as f64;
    if delta_rating <= 0.0 {
        return None;
    }

    let scalar_weighted = infer_scalar_weighted(delta_rating, delta_secs, current.territory_count)?;
    let scalar_raw = infer_scalar_raw(delta_rating, delta_secs, current.territory_count)?;
    if !(0.0..=MAX_REASONABLE_SCALAR).contains(&scalar_weighted) {
        return None;
    }
    if !(0.0..=MAX_REASONABLE_SCALAR).contains(&scalar_raw) {
        return None;
    }

    Some(ScalarEstimate {
        season_id: current.season_id,
        scalar_weighted,
        scalar_raw,
    })
}

fn select_dominant_season(estimates: Vec<ScalarEstimate>) -> Option<(i32, Vec<ScalarEstimate>)> {
    let mut grouped: HashMap<i32, Vec<ScalarEstimate>> = HashMap::new();
    for estimate in estimates {
        grouped
            .entry(estimate.season_id)
            .or_default()
            .push(estimate);
    }

    grouped
        .into_iter()
        .max_by(|(season_a, vec_a), (season_b, vec_b)| {
            vec_a
                .len()
                .cmp(&vec_b.len())
                .then_with(|| season_a.cmp(season_b))
        })
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        Some((values[mid - 1] + values[mid]) / 2.0)
    } else {
        Some(values[mid])
    }
}

fn percentile(sorted_values: &[f64], p: f64) -> f64 {
    if sorted_values.is_empty() {
        return 0.0;
    }
    if sorted_values.len() == 1 {
        return sorted_values[0];
    }
    let idx = p.clamp(0.0, 1.0) * (sorted_values.len() as f64 - 1.0);
    let low = idx.floor() as usize;
    let high = idx.ceil() as usize;
    if low == high {
        return sorted_values[low];
    }
    let frac = idx - low as f64;
    sorted_values[low] + (sorted_values[high] - sorted_values[low]) * frac
}

fn iqr(sorted_values: &[f64]) -> f64 {
    if sorted_values.len() < 2 {
        return 0.0;
    }
    let p25 = percentile(sorted_values, 0.25);
    let p75 = percentile(sorted_values, 0.75);
    (p75 - p25).abs()
}

fn compute_confidence(sample_count: usize, median_weighted: f64, spread: f64) -> f64 {
    if sample_count == 0 {
        return 0.0;
    }
    let sample_factor = (sample_count as f64 / SCALAR_CANDIDATE_GUILDS as f64).clamp(0.0, 1.0);
    let baseline = median_weighted.abs().max(1e-6);
    let normalized_spread = spread / baseline;
    let spread_factor = (1.0 / (1.0 + normalized_spread * 6.0)).clamp(0.0, 1.0);
    (sample_factor * spread_factor).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::{
        GuildObservation, GuildSeasonRank, GuildSeasonSnapshot, compute_confidence,
        derive_scalar_estimate, latest_season_rating, select_dominant_season,
    };
    use chrono::{TimeDelta, Utc};
    use std::collections::HashMap;

    #[test]
    fn latest_season_rating_uses_highest_season_key() {
        let ranks = HashMap::from([
            ("28".to_string(), GuildSeasonRank { rating: 150 }),
            ("29".to_string(), GuildSeasonRank { rating: 420 }),
            ("10".to_string(), GuildSeasonRank { rating: 99 }),
        ]);
        let latest = latest_season_rating(&ranks).expect("latest season should parse");
        assert_eq!(latest.0, 29);
        assert_eq!(latest.1, 420);
    }

    #[test]
    fn derive_scalar_estimate_skips_on_season_or_territory_change() {
        let now = Utc::now();
        let previous = GuildObservation {
            observed_at: now - TimeDelta::minutes(5),
            season_id: 29,
            rating: 1000,
            territory_count: 5,
        };
        let changed_season = GuildSeasonSnapshot {
            guild_name: "Guild".to_string(),
            guild_uuid: "uuid".to_string(),
            guild_prefix: "TAG".to_string(),
            observed_at: now,
            season_id: 30,
            rating: 1100,
            territory_count: 5,
        };
        let changed_territories = GuildSeasonSnapshot {
            guild_name: "Guild".to_string(),
            guild_uuid: "uuid".to_string(),
            guild_prefix: "TAG".to_string(),
            observed_at: now,
            season_id: 29,
            rating: 1100,
            territory_count: 6,
        };

        assert!(derive_scalar_estimate(&previous, &changed_season).is_none());
        assert!(derive_scalar_estimate(&previous, &changed_territories).is_none());
    }

    #[test]
    fn select_dominant_season_picks_largest_group_then_latest_key() {
        let dominant = select_dominant_season(vec![
            super::ScalarEstimate {
                season_id: 29,
                scalar_weighted: 1.2,
                scalar_raw: 1.1,
            },
            super::ScalarEstimate {
                season_id: 29,
                scalar_weighted: 1.4,
                scalar_raw: 1.3,
            },
            super::ScalarEstimate {
                season_id: 28,
                scalar_weighted: 0.8,
                scalar_raw: 0.9,
            },
        ])
        .expect("dominant season should exist");

        assert_eq!(dominant.0, 29);
        assert_eq!(dominant.1.len(), 2);
    }

    #[test]
    fn compute_confidence_increases_with_more_samples_and_lower_spread() {
        let low = compute_confidence(1, 2.0, 1.2);
        let high = compute_confidence(8, 2.0, 0.05);
        assert!(high > low);
        assert!((0.0..=1.0).contains(&low));
        assert!((0.0..=1.0).contains(&high));
    }
}
