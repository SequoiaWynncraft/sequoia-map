use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tracing::{info, warn};

use crate::config::{TERREXTRA_REFRESH_SECS, TERREXTRA_URL};
use crate::state::{AppState, ExtraTerrInfo};

pub async fn run(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(TERREXTRA_REFRESH_SECS));

    // Fetch immediately on startup, then hourly
    loop {
        interval.tick().await;

        match fetch_extra_data(&state.http_client).await {
            Ok(data) => {
                let count = data.len();
                *state.extra_terr.write().await = data;
                state.extra_data_dirty.store(true, Ordering::Release);
                info!("loaded extra territory data for {count} territories");
            }
            Err(e) => {
                warn!("failed to fetch terrextra.json: {e}");
            }
        }
    }
}

async fn fetch_extra_data(
    client: &reqwest::Client,
) -> Result<HashMap<String, ExtraTerrInfo>, reqwest::Error> {
    let resp = client.get(TERREXTRA_URL).send().await?;
    let data: HashMap<String, ExtraTerrInfo> = resp.json().await?;
    Ok(data)
}
