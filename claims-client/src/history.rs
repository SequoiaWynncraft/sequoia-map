use leptos::prelude::*;
use sequoia_shared::LiveState;

use crate::app::BufferedUpdate;

const MAX_BUFFERED_UPDATES: usize = 20_000;

pub async fn fetch_live_state() -> Result<LiveState, String> {
    let resp = gloo_net::http::Request::get("/api/live/state")
        .send()
        .await
        .map_err(|e| format!("fetch error: {e}"))?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    resp.json::<LiveState>()
        .await
        .map_err(|e| format!("parse error: {e}"))
}

pub fn buffer_history_update(
    history_buffered_updates: RwSignal<Vec<BufferedUpdate>>,
    history_buffer_size_max: RwSignal<usize>,
    needs_live_resync: RwSignal<bool>,
    update: BufferedUpdate,
) {
    let mut overflowed = false;
    let mut new_len = 0;

    history_buffered_updates.update(|buffer| {
        if buffer.iter().any(|existing| existing.seq == update.seq) {
            new_len = buffer.len();
            return;
        }

        buffer.push(update);
        buffer.sort_by_key(|item| item.seq);

        if buffer.len() > MAX_BUFFERED_UPDATES {
            let overflow = buffer.len() - MAX_BUFFERED_UPDATES;
            buffer.drain(0..overflow);
            overflowed = true;
        }

        new_len = buffer.len();
    });

    if overflowed {
        needs_live_resync.set(true);
        web_sys::console::warn_1(&"claims live buffer overflowed; forcing live resync".into());
    }

    history_buffer_size_max.update(|current_max| {
        if new_len > *current_max {
            *current_max = new_len;
        }
    });
}

pub fn has_seq_gap(last_live_seq: Option<u64>, incoming_seq: u64) -> bool {
    if incoming_seq == 0 {
        return false;
    }

    match last_live_seq {
        Some(last_seq) => incoming_seq != last_seq.saturating_add(1),
        None => false,
    }
}
