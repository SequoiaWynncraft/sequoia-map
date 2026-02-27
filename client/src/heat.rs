use chrono::SecondsFormat;
use sequoia_shared::history::{HistoryHeat, HistoryHeatMeta, HistoryHeatSource};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeatFetchInput {
    pub source: HistoryHeatSource,
    pub season_id: Option<i32>,
    pub at: Option<i64>,
}

pub fn build_heat_query(input: HeatFetchInput) -> Result<String, String> {
    let mut params = vec![format!(
        "source={}",
        match input.source {
            HistoryHeatSource::Season => "season",
            HistoryHeatSource::AllTime => "all_time",
        }
    )];

    if matches!(input.source, HistoryHeatSource::Season)
        && let Some(season_id) = input.season_id
    {
        params.push(format!("season_id={season_id}"));
    }

    if let Some(at) = input.at {
        let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(at, 0)
            .ok_or_else(|| format!("invalid timestamp: {at}"))?;
        let encoded = dt.to_rfc3339_opts(SecondsFormat::Secs, true);
        params.push(format!("at={encoded}"));
    }

    Ok(params.join("&"))
}

pub async fn fetch_heat_meta() -> Result<HistoryHeatMeta, String> {
    let resp = gloo_net::http::Request::get("/api/history/heat/meta")
        .send()
        .await
        .map_err(|e| format!("fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<HistoryHeatMeta>()
        .await
        .map_err(|e| format!("parse error: {e}"))
}

pub async fn fetch_heat(input: HeatFetchInput) -> Result<HistoryHeat, String> {
    let query = build_heat_query(input)?;
    let url = format!("/api/history/heat?{query}");
    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<HistoryHeat>()
        .await
        .map_err(|e| format!("parse error: {e}"))
}

#[cfg(any(target_arch = "wasm32", test))]
fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
    let t = t.clamp(0.0, 1.0);
    let value = a as f64 + (b as f64 - a as f64) * t;
    value.round().clamp(0.0, 255.0) as u8
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn heat_color_for_intensity(intensity: f64) -> (u8, u8, u8) {
    const STOPS: &[(f64, (u8, u8, u8))] = &[
        (0.00, (30, 80, 220)),
        (0.25, (40, 200, 240)),
        (0.50, (245, 220, 70)),
        (0.75, (245, 140, 50)),
        (1.00, (220, 40, 35)),
    ];

    let intensity = intensity.clamp(0.0, 1.0);
    for window in STOPS.windows(2) {
        let (left_pos, left_color) = window[0];
        let (right_pos, right_color) = window[1];
        if intensity >= left_pos && intensity <= right_pos {
            let span = (right_pos - left_pos).max(f64::EPSILON);
            let t = (intensity - left_pos) / span;
            return (
                lerp_u8(left_color.0, right_color.0, t),
                lerp_u8(left_color.1, right_color.1, t),
                lerp_u8(left_color.2, right_color.2, t),
            );
        }
    }

    STOPS
        .last()
        .map(|(_, color)| *color)
        .unwrap_or((220, 40, 35))
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn heat_color_for_count(take_count: u64, max_take_count: u64) -> (u8, u8, u8) {
    if max_take_count == 0 {
        return heat_color_for_intensity(0.0);
    }
    let intensity = (take_count as f64 / max_take_count as f64).clamp(0.0, 1.0);
    heat_color_for_intensity(intensity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_heat_query_for_season_total() {
        let query = build_heat_query(HeatFetchInput {
            source: HistoryHeatSource::Season,
            season_id: Some(29),
            at: None,
        })
        .expect("query should build");
        assert_eq!(query, "source=season&season_id=29");
    }

    #[test]
    fn build_heat_query_for_all_time_cumulative() {
        let query = build_heat_query(HeatFetchInput {
            source: HistoryHeatSource::AllTime,
            season_id: None,
            at: Some(1_700_000_000),
        })
        .expect("query should build");
        assert!(query.starts_with("source=all_time&at="));
    }

    #[test]
    fn heat_color_handles_zero_max() {
        assert_eq!(heat_color_for_count(0, 0), (30, 80, 220));
        assert_eq!(heat_color_for_count(10, 0), (30, 80, 220));
    }

    #[test]
    fn heat_color_matches_gradient_edges() {
        assert_eq!(heat_color_for_intensity(0.0), (30, 80, 220));
        assert_eq!(heat_color_for_intensity(0.5), (245, 220, 70));
        assert_eq!(heat_color_for_intensity(1.0), (220, 40, 35));
    }
}
