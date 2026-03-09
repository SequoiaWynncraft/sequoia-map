use std::collections::HashMap;

use leptos::prelude::*;
use sequoia_shared::TerritoryChange;

use crate::claims::ClaimsPage;

pub(crate) fn canvas_dimensions() -> (f64, f64) {
    let Some(window) = web_sys::window() else {
        return (1200.0, 800.0);
    };
    let w = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1200.0);
    let h = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(800.0);
    (w, h)
}

pub(crate) fn set_loading_shell_step(step: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    if let Some(step_el) = document.get_element_by_id("app-loading-step") {
        step_el.set_text_content(Some(step));
    }
}

pub(crate) fn remove_loading_shell() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    if let Some(shell) = document.get_element_by_id("app-loading-shell") {
        shell.remove();
    }
}

pub(crate) const MOBILE_BREAKPOINT: f64 = 768.0;

#[derive(Clone, Copy)]
pub(crate) struct Hovered(pub RwSignal<Option<String>>);
#[derive(Clone, Copy)]
pub(crate) struct Selected(pub RwSignal<Option<String>>);
#[derive(Clone, Copy)]
pub(crate) struct AbbreviateNames(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowCountdown(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowGranularMapTime(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowCompoundMapTime(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowNames(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ThickCooldownBorders(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct BoldConnections(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ConnectionOpacityScale(pub RwSignal<f64>);
#[derive(Clone, Copy)]
pub(crate) struct ConnectionThicknessScale(pub RwSignal<f64>);
#[derive(Clone, Copy)]
pub(crate) struct ResourceHighlight(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowResourceIcons(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowTerritoryOrnaments(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct NameColorSetting(pub RwSignal<NameColor>);
#[derive(Clone, Copy)]
pub(crate) struct TagColorSetting(pub RwSignal<NameColor>);
#[derive(Clone, Copy)]
pub(crate) struct ReadableFont(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowMinimap(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct LabelScaleMaster(pub RwSignal<f64>);
#[derive(Clone, Copy)]
pub(crate) struct LabelScaleStatic(pub RwSignal<f64>);
#[derive(Clone, Copy)]
pub(crate) struct LabelScaleStaticName(pub RwSignal<f64>);
#[derive(Clone, Copy)]
pub(crate) struct LabelScaleDynamic(pub RwSignal<f64>);
#[derive(Clone, Copy)]
pub(crate) struct LabelScaleIcons(pub RwSignal<f64>);
#[derive(Clone, Copy)]
pub(crate) struct SidebarOpen(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct SidebarTransient(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowSettings(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct IsMobile(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct PeekTerritory(pub RwSignal<Option<String>>);
#[derive(Clone, Copy)]
pub(crate) struct DetailReturnGuild(pub RwSignal<Option<String>>);
#[derive(Clone, Copy)]
pub(crate) struct HeatModeEnabled(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct HeatEntriesByTerritory(pub RwSignal<HashMap<String, u64>>);
#[derive(Clone, Copy)]
pub(crate) struct HeatMaxTakeCount(pub RwSignal<u64>);
#[derive(Clone, Copy)]
pub(crate) struct HeatWindowLabel(pub RwSignal<String>);
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapMode {
    Live,
    History,
}
#[derive(Clone, Copy)]
pub(crate) struct CurrentMode(pub RwSignal<MapMode>);
#[derive(Clone, Copy)]
pub(crate) struct HistoryTimestamp(pub RwSignal<Option<i64>>);
#[derive(Clone, Copy)]
pub(crate) struct HistoryFetchNonce(pub RwSignal<u64>);
#[derive(Clone, Copy)]
pub(crate) struct LastLiveSeq(pub RwSignal<Option<u64>>);
#[derive(Clone, Copy)]
pub(crate) struct HistoryBufferedUpdates(pub RwSignal<Vec<BufferedUpdate>>);
#[derive(Clone, Copy)]
pub(crate) struct HistoryBufferModeActive(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct NeedsLiveResync(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct LiveResyncInFlight(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct SseSeqGapDetectedCount(pub RwSignal<u64>);
#[derive(Clone, Copy)]
pub(crate) struct HistoryBufferSizeMax(pub RwSignal<usize>);

#[derive(Clone, Debug)]
pub(crate) struct BufferedUpdate {
    pub seq: u64,
    pub changes: Vec<TerritoryChange>,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum NameColor {
    White,
    Guild,
    Gold,
    Copper,
    Muted,
}

fn browser_pathname() -> String {
    web_sys::window()
        .and_then(|window| window.location().pathname().ok())
        .unwrap_or_else(|| "/claims".to_string())
}

#[component]
pub fn App() -> impl IntoView {
    let pathname = browser_pathname();
    view! { <ClaimsPage initial_path=pathname /> }
}
