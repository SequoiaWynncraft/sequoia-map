use leptos::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;

use crate::app::{
    CurrentMode, GuildColorStore, HistoryBoundsSignal, HistoryBufferModeActive,
    HistoryBufferedUpdates, HistoryFetchNonce, HistoryTimestamp, LastLiveSeq,
    LiveHandoffResyncCount, MapMode, NeedsLiveResync, PlaybackActive, PlaybackSpeed, SidebarOpen,
    TerritoryGeometryStore,
};
use crate::history;
use crate::territory::ClientTerritoryMap;
use gloo_timers::callback::Timeout;

/// Timeline scrubber bar, visible only in history mode.
#[component]
pub fn Timeline() -> impl IntoView {
    let CurrentMode(mode) = expect_context();
    let HistoryTimestamp(timestamp) = expect_context();
    let PlaybackActive(playing) = expect_context();
    let PlaybackSpeed(speed) = expect_context();
    let HistoryBoundsSignal(bounds) = expect_context();
    let HistoryFetchNonce(history_fetch_nonce) = expect_context();
    let HistoryBufferedUpdates(history_buffered_updates) = expect_context();
    let HistoryBufferModeActive(history_buffer_mode_active) = expect_context();
    let LastLiveSeq(last_live_seq) = expect_context();
    let NeedsLiveResync(needs_live_resync) = expect_context();
    let LiveHandoffResyncCount(live_handoff_resync_count) = expect_context();
    let territories: RwSignal<ClientTerritoryMap> = expect_context();
    let TerritoryGeometryStore(geo_store) = expect_context();
    let GuildColorStore(guild_color_store) = expect_context();
    let SidebarOpen(sidebar_open) = expect_context();
    let is_visible = move || mode.get() == MapMode::History;

    // Debounce timer for scrubbing.
    // Hold the timeout handle so we can cancel without leaking JS callbacks.
    let debounce_timeout = Rc::new(RefCell::new(None::<Timeout>));

    let on_range_input = {
        let debounce_timeout = Rc::clone(&debounce_timeout);
        move |e: web_sys::Event| {
            let Some(target) = e.target() else {
                return;
            };
            let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() else {
                return;
            };
            let val: i64 = input.value().parse().unwrap_or(0);

            // Update timestamp immediately for visual feedback
            playing.set(false);
            timestamp.set(Some(val));

            // Debounce the actual fetch.
            if let Some(timeout) = debounce_timeout.borrow_mut().take() {
                timeout.cancel();
            }

            let timeout = Timeout::new(150, move || {
                history::fetch_and_apply_with(
                    val,
                    mode,
                    history_fetch_nonce,
                    geo_store,
                    guild_color_store,
                    territories,
                );
            });
            *debounce_timeout.borrow_mut() = Some(timeout);
        }
    };

    let format_timestamp = move |ts: i64| -> String {
        chrono::DateTime::from_timestamp(ts, 0)
            .map(|dt| dt.format("%b %d %H:%M").to_string())
            .unwrap_or_default()
    };

    let speed_options = [1.0, 10.0, 60.0, 360.0];

    // SVG icon constants
    let play_svg = r#"<svg width="12" height="14" viewBox="0 0 12 14" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M1 1.5v11l10-5.5z"/></svg>"#;
    let pause_svg = r#"<svg width="12" height="14" viewBox="0 0 12 14" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><rect x="1" y="1" width="3.5" height="12" rx="0.75"/><rect x="7.5" y="1" width="3.5" height="12" rx="0.75"/></svg>"#;
    let skip_back_svg = r#"<svg width="14" height="12" viewBox="0 0 14 12" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><rect x="1" y="1" width="2" height="10" rx="0.5"/><path d="M13 1v10L5.5 6z"/></svg>"#;
    let skip_fwd_svg = r#"<svg width="14" height="12" viewBox="0 0 14 12" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><rect x="11" y="1" width="2" height="10" rx="0.5"/><path d="M1 1v10l7.5-5z"/></svg>"#;

    view! {
        <div
            style:display=move || if is_visible() { "flex" } else { "none" }
            style:right=move || if sidebar_open.get() { "340px" } else { "0" }
            style="position: absolute; bottom: 0; left: 0; z-index: 25; height: 52px; background: #13161f; border-top: 1px solid rgba(245,197,66,0.15); align-items: center; gap: 0; padding: 0 16px; font-family: 'JetBrains Mono', monospace; font-size: 0.72rem; transition: right 0.3s cubic-bezier(0.4, 0, 0.2, 1);"
        >
            // — Transport controls group —
            // Play/Pause button
            <button
                title=move || if playing.get() { "Pause (Space)" } else { "Play (Space)" }
                style="width: 32px; height: 32px; background: #1a1d2a; border: 1px solid #282c3e; border-radius: 6px; cursor: pointer; color: #f5c542; font-size: 0.9rem; display: flex; align-items: center; justify-content: center; flex-shrink: 0; transition: background 0.15s ease, border-color 0.15s ease;"
                inner_html=move || if playing.get() { pause_svg } else { play_svg }
                on:click=move |_| playing.update(|v| *v = !*v)
                on:mouseenter=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("background", "#232738").ok();
                        el.style().set_property("border-color", "#3a3f5c").ok();
                    }
                }
                on:mouseleave=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("background", "#1a1d2a").ok();
                        el.style().set_property("border-color", "#282c3e").ok();
                    }
                }
            />

            // Step backward
            <button
                title="Step back ([)"
                style="width: 28px; height: 28px; background: #1a1d2a; border: 1px solid #282c3e; border-radius: 4px; cursor: pointer; color: #9a9590; font-size: 0.75rem; display: flex; align-items: center; justify-content: center; flex-shrink: 0; margin-left: 4px; transition: background 0.15s ease, color 0.15s ease, border-color 0.15s ease;"
                inner_html=skip_back_svg
                on:click=move |_| {
                    history::step_backward(
                        timestamp,
                        playing,
                        mode,
                        history_fetch_nonce,
                        geo_store,
                        guild_color_store,
                        territories,
                    );
                }
                on:mouseenter=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("background", "#232738").ok();
                        el.style().set_property("color", "#e2e0d8").ok();
                        el.style().set_property("border-color", "#3a3f5c").ok();
                    }
                }
                on:mouseleave=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("background", "#1a1d2a").ok();
                        el.style().set_property("color", "#9a9590").ok();
                        el.style().set_property("border-color", "#282c3e").ok();
                    }
                }
            />

            // Step forward
            <button
                title="Step forward (])"
                style="width: 28px; height: 28px; background: #1a1d2a; border: 1px solid #282c3e; border-radius: 4px; cursor: pointer; color: #9a9590; font-size: 0.75rem; display: flex; align-items: center; justify-content: center; flex-shrink: 0; margin-left: 4px; transition: background 0.15s ease, color 0.15s ease, border-color 0.15s ease;"
                inner_html=skip_fwd_svg
                on:click=move |_| {
                    history::step_forward(
                        timestamp,
                        playing,
                        mode,
                        history_fetch_nonce,
                        geo_store,
                        guild_color_store,
                        territories,
                    );
                }
                on:mouseenter=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("background", "#232738").ok();
                        el.style().set_property("color", "#e2e0d8").ok();
                        el.style().set_property("border-color", "#3a3f5c").ok();
                    }
                }
                on:mouseleave=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("background", "#1a1d2a").ok();
                        el.style().set_property("color", "#9a9590").ok();
                        el.style().set_property("border-color", "#282c3e").ok();
                    }
                }
            />

            // Divider: transport | speed
            <div style="width: 1px; height: 24px; background: #282c3e; margin: 0 10px; flex-shrink: 0;" />

            // Speed selector
            <select
                prop:value=move || speed.get().to_string()
                style="background: #1a1d2a; border: 1px solid #282c3e; border-radius: 4px; color: #9a9590; font-size: 0.68rem; padding: 4px 6px; cursor: pointer; font-family: 'JetBrains Mono', monospace; flex-shrink: 0; outline: none; transition: border-color 0.15s ease, color 0.15s ease;"
                on:change=move |e| {
                    let Some(target) = e.target() else {
                        return;
                    };
                    let Ok(target) = target.dyn_into::<web_sys::HtmlSelectElement>() else {
                        return;
                    };
                    if let Ok(val) = target.value().parse::<f64>() {
                        speed.set(val);
                    }
                }
                on:mouseenter=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("border-color", "#3a3f5c").ok();
                        el.style().set_property("color", "#e2e0d8").ok();
                    }
                }
                on:mouseleave=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("border-color", "#282c3e").ok();
                        el.style().set_property("color", "#9a9590").ok();
                    }
                }
            >
                {speed_options.iter().map(|&s| {
                    let label = format!("{}x", s as i32);
                    let val = format!("{s}");
                    view! {
                        <option value=val>{label}</option>
                    }
                }).collect::<Vec<_>>()}
            </select>

            // Divider: speed | timeline
            <div style="width: 1px; height: 24px; background: #282c3e; margin: 0 10px; flex-shrink: 0;" />

            // Left timestamp label
            <span style="color: #5a5860; flex-shrink: 0; font-size: 0.65rem;">
                {move || bounds.get().map(|(earliest, _)| format_timestamp(earliest)).unwrap_or_default()}
            </span>

            // Timeline range slider
            <input
                type="range"
                class="timeline-slider"
                style="flex: 1; margin: 0 8px;"
                min=move || bounds.get().map(|(e, _)| e.to_string()).unwrap_or_else(|| "0".to_string())
                max=move || bounds.get().map(|(_, l)| l.to_string()).unwrap_or_else(|| "0".to_string())
                value=move || timestamp.get().unwrap_or(0).to_string()
                on:input=on_range_input
            />

            // Right timestamp label
            <span style="color: #5a5860; flex-shrink: 0; font-size: 0.65rem;">
                {move || bounds.get().map(|(_, latest)| format_timestamp(latest)).unwrap_or_default()}
            </span>

            // Divider: timeline | current time
            <div style="width: 1px; height: 24px; background: #282c3e; margin: 0 10px; flex-shrink: 0;" />

            // Current time display
            <span style="color: #e2e0d8; flex-shrink: 0; min-width: 100px; text-align: center; font-size: 0.7rem;">
                {move || timestamp.get().map(&format_timestamp).unwrap_or_default()}
            </span>

            // Divider: current time | mode indicator
            <div style="width: 1px; height: 24px; background: #282c3e; margin: 0 10px; flex-shrink: 0;" />

            // Mode indicator
            <span
                style="border: 1px solid #3a3f5c; border-radius: 4px; padding: 5px 12px; color: #9a9590; font-family: 'JetBrains Mono', monospace; font-size: 0.68rem; font-weight: 700; flex-shrink: 0; letter-spacing: 0.05em;"
            >
                "History"
            </span>

            // Divider: mode indicator | live action
            <div style="width: 1px; height: 24px; background: #282c3e; margin: 0 10px; flex-shrink: 0;" />

            // Live action
            <button
                title="Switch to live mode (h)"
                style:background="#f5c542"
                style:color="#13161f"
                style:border-color="#f5c542"
                style:box-shadow="0 0 10px rgba(245,197,66,0.35)"
                style="border: 1px solid; border-radius: 4px; padding: 5px 12px; cursor: pointer; font-family: 'JetBrains Mono', monospace; font-size: 0.68rem; font-weight: 700; flex-shrink: 0; letter-spacing: 0.05em; transition: all 0.15s ease;"
                on:click=move |_| {
                    if mode.get_untracked() == MapMode::History {
                        history::exit_history_mode(history::ExitHistoryModeInput {
                            mode,
                            playback_active: playing,
                            history_fetch_nonce,
                            history_timestamp: timestamp,
                            history_buffered_updates,
                            history_buffer_mode_active,
                            last_live_seq,
                            needs_live_resync,
                            live_handoff_resync_count,
                            territories,
                        });
                    }
                }
                on:mouseenter=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("background", "#ffd86f").ok();
                        el.style().set_property("border-color", "#ffd86f").ok();
                    }
                }
                on:mouseleave=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("background", "#f5c542").ok();
                        el.style().set_property("border-color", "#f5c542").ok();
                    }
                }
            >
                "Go Live"
            </button>
        </div>
    }
}
