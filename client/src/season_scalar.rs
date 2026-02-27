use crate::app::MapMode;
use sequoia_shared::{SeasonScalarCurrent, SeasonScalarSample};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarSource {
    Manual,
    LiveEstimate,
    HistoryEstimate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveScalar {
    pub value: f64,
    pub source: ScalarSource,
    pub sample: Option<SeasonScalarSample>,
}

pub fn clamp_manual_scalar(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return 1.5;
    }
    value.clamp(0.05, 20.0)
}

pub fn effective_scalar(
    mode: MapMode,
    auto_enabled: bool,
    manual_scalar: f64,
    live_sample: Option<SeasonScalarSample>,
    history_sample: Option<SeasonScalarSample>,
) -> EffectiveScalar {
    let manual = clamp_manual_scalar(manual_scalar);
    if !auto_enabled {
        return EffectiveScalar {
            value: manual,
            source: ScalarSource::Manual,
            sample: None,
        };
    }

    match mode {
        MapMode::Live => {
            if let Some(sample) = live_sample {
                EffectiveScalar {
                    value: sample.scalar_weighted,
                    source: ScalarSource::LiveEstimate,
                    sample: Some(sample),
                }
            } else {
                EffectiveScalar {
                    value: manual,
                    source: ScalarSource::Manual,
                    sample: None,
                }
            }
        }
        MapMode::History => {
            if let Some(sample) = history_sample {
                EffectiveScalar {
                    value: sample.scalar_weighted,
                    source: ScalarSource::HistoryEstimate,
                    sample: Some(sample),
                }
            } else {
                EffectiveScalar {
                    value: manual,
                    source: ScalarSource::Manual,
                    sample: None,
                }
            }
        }
    }
}

pub async fn fetch_current_scalar_sample() -> Result<Option<SeasonScalarSample>, String> {
    let response = gloo_net::http::Request::get("/api/season/scalar/current")
        .send()
        .await
        .map_err(|e| format!("fetch error: {e}"))?;

    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }

    let payload = response
        .json::<SeasonScalarCurrent>()
        .await
        .map_err(|e| format!("parse error: {e}"))?;
    Ok(payload.sample)
}

#[cfg(test)]
mod tests {
    use super::{ScalarSource, clamp_manual_scalar, effective_scalar};
    use crate::app::MapMode;
    use sequoia_shared::SeasonScalarSample;

    fn sample(weighted: f64, raw: f64) -> SeasonScalarSample {
        SeasonScalarSample {
            sampled_at: "2026-02-25T00:00:00Z".to_string(),
            season_id: 29,
            scalar_weighted: weighted,
            scalar_raw: raw,
            confidence: 0.8,
            sample_count: 5,
        }
    }

    #[test]
    fn effective_scalar_prefers_manual_when_auto_disabled() {
        let result = effective_scalar(
            MapMode::Live,
            false,
            1.9,
            Some(sample(2.0, 2.3)),
            Some(sample(1.7, 2.0)),
        );
        assert_eq!(result.source, ScalarSource::Manual);
        assert!((result.value - 1.9).abs() < 1e-9);
        assert!(result.sample.is_none());
    }

    #[test]
    fn effective_scalar_uses_live_sample_in_live_mode() {
        let result = effective_scalar(
            MapMode::Live,
            true,
            1.5,
            Some(sample(2.4, 2.8)),
            Some(sample(1.9, 2.2)),
        );
        assert_eq!(result.source, ScalarSource::LiveEstimate);
        assert!((result.value - 2.4).abs() < 1e-9);
        assert_eq!(result.sample.expect("sample").scalar_raw, 2.8);
    }

    #[test]
    fn effective_scalar_uses_history_sample_in_history_mode() {
        let result = effective_scalar(
            MapMode::History,
            true,
            1.5,
            Some(sample(2.4, 2.8)),
            Some(sample(1.8, 2.0)),
        );
        assert_eq!(result.source, ScalarSource::HistoryEstimate);
        assert!((result.value - 1.8).abs() < 1e-9);
        assert_eq!(result.sample.expect("sample").scalar_raw, 2.0);
    }

    #[test]
    fn effective_scalar_falls_back_to_manual_if_sample_missing() {
        let result = effective_scalar(MapMode::History, true, 1.7, Some(sample(2.4, 2.8)), None);
        assert_eq!(result.source, ScalarSource::Manual);
        assert!((result.value - 1.7).abs() < 1e-9);
    }

    #[test]
    fn clamp_manual_scalar_clamps_and_sanitizes() {
        assert!((clamp_manual_scalar(3.2) - 3.2).abs() < 1e-9);
        assert!((clamp_manual_scalar(0.0) - 1.5).abs() < 1e-9);
        assert!((clamp_manual_scalar(f64::NAN) - 1.5).abs() < 1e-9);
        assert!((clamp_manual_scalar(99.0) - 20.0).abs() < 1e-9);
    }
}
