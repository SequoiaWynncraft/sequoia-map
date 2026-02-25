use js_sys::{Function, Reflect};
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

use std::cell::RefCell;
use std::collections::HashMap;

pub(crate) const SIDEBAR_WIDTH: f64 = 340.0;

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

fn set_loading_shell_step(step: &str) {
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

fn remove_loading_shell() {
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

struct TickIntervalBinding {
    window: web_sys::Window,
    interval_id: i32,
    _callback: wasm_bindgen::closure::Closure<dyn Fn()>,
}

struct KeydownBinding {
    window: web_sys::Window,
    _handler: wasm_bindgen::closure::Closure<dyn Fn(web_sys::KeyboardEvent)>,
}

thread_local! {
    static TICK_INTERVAL_BINDING: RefCell<Option<TickIntervalBinding>> = const { RefCell::new(None) };
    static KEYDOWN_BINDING: RefCell<Option<KeydownBinding>> = const { RefCell::new(None) };
}

use sequoia_shared::{Region, Resources, TerritoryChange, TreasuryLevel};

/// Newtype wrappers to give `hovered` and `selected` distinct types for Leptos context.
/// (Both are `RwSignal<Option<String>>` — without wrappers, `provide_context` overwrites one.)
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
pub(crate) struct ShowNames(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ThickCooldownBorders(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct BoldNames(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct BoldTags(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ThickTagOutline(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ThickNameOutline(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ReadableFont(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct BoldConnections(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ResourceHighlight(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct SidebarOpen(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct SidebarIndex(pub RwSignal<usize>);
#[derive(Clone, Copy)]
pub(crate) struct SidebarItems(pub RwSignal<Vec<String>>);

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
pub(crate) struct PlaybackActive(pub RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct PlaybackSpeed(pub RwSignal<f64>);
#[derive(Clone, Copy)]
pub(crate) struct HistoryBoundsSignal(pub RwSignal<Option<(i64, i64)>>);
#[derive(Clone, Copy)]
pub(crate) struct HistoryAvailable(pub RwSignal<bool>);
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
pub(crate) struct LiveHandoffResyncCount(pub RwSignal<u64>);
#[derive(Clone, Copy)]
pub(crate) struct SseSeqGapDetectedCount(pub RwSignal<u64>);
#[derive(Clone, Copy)]
pub(crate) struct HistoryBufferSizeMax(pub RwSignal<usize>);

#[derive(Clone, Debug)]
pub(crate) struct BufferedUpdate {
    pub seq: u64,
    pub changes: Vec<TerritoryChange>,
}

pub(crate) type TerritoryGeometry = (Region, Resources, Vec<String>);
pub(crate) type TerritoryGeometryMap = HashMap<String, TerritoryGeometry>;
pub(crate) type GuildColorMap = HashMap<String, (u8, u8, u8)>;

/// Immutable snapshot of territory geometry captured when entering history mode.
/// Prevents history/playback operations from degrading geometry data.
#[derive(Clone, Copy)]
pub(crate) struct TerritoryGeometryStore(pub StoredValue<TerritoryGeometryMap>);
#[derive(Clone, Copy)]
pub(crate) struct GuildColorStore(pub StoredValue<GuildColorMap>);

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum NameColor {
    White,  // rgba(220, 218, 210, 0.88) — current default
    Guild,  // per-territory guild color (brightened), same as tag line
    Gold,   // rgba(245, 197, 66, 0.88) — matches app accent
    Copper, // rgba(181, 103, 39, 0.88) — warm copper
    Muted,  // rgba(120, 116, 112, 0.78) — subtle/subdued
}

#[derive(Clone, Copy)]
pub(crate) struct NameColorSetting(pub RwSignal<NameColor>);

use gloo_storage::Storage;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Settings {
    show_connections: bool,
    abbreviate_names: bool,
    show_countdown: bool,
    granular_map_time: bool,
    show_names: bool,
    thick_cooldown_borders: bool,
    bold_names: bool,
    bold_tags: bool,
    thick_tag_outline: bool,
    thick_name_outline: bool,
    readable_font: bool,
    bold_connections: bool,
    name_color: NameColor,
    sidebar_open: bool,
    resource_highlight: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_connections: true,
            abbreviate_names: true,
            show_countdown: false,
            granular_map_time: false,
            show_names: true,
            thick_cooldown_borders: true,
            bold_names: false,
            bold_tags: false,
            thick_tag_outline: false,
            thick_name_outline: false,
            readable_font: false,
            bold_connections: false,
            name_color: NameColor::White,
            sidebar_open: false,
            resource_highlight: false,
        }
    }
}

use crate::canvas::{MapCanvas, abbreviate_name};
use crate::colors::rgba_css;
use crate::history;
use crate::icons::{self, ResourceIcons};
use crate::minimap::Minimap;
use crate::sidebar::Sidebar;
use crate::sse::{self, ConnectionStatus};
use crate::territory::ClientTerritoryMap;
use crate::tiles::{self, LoadedTile};
use crate::time_format::format_hms;
use crate::timeline::Timeline;
use crate::viewport::Viewport;

/// Format a resource value for compact display (e.g. 9000 -> "9.0k").
fn format_resource_compact(val: i32) -> String {
    if val >= 1000 {
        format!("{:.1}k", val as f64 / 1000.0)
    } else {
        format!("{val}")
    }
}

/// Build inline HTML with `<img>` icon tags and amounts for resource indicators in tooltips.
fn resource_icons_html(res: &Resources) -> String {
    let mut html = String::new();
    if res.has_all() {
        html.push_str(r#"<span style="display:inline-flex;align-items:center;gap:3px;background:#1a1d2a;padding:1px 5px;border-radius:3px;border:1px solid #282c3e;"><img src="/icons/rainbow.svg" style="width:11px;height:11px;vertical-align:middle;image-rendering:pixelated;" /><span style="font-size:0.6rem;color:#e2e0d8;">All</span></span>"#);
        return html;
    }
    let items: &[(i32, bool, &str, &str)] = &[
        (
            res.emeralds,
            res.has_double_emeralds(),
            "emerald",
            "Emeralds",
        ),
        (res.ore, res.has_double_ore(), "ore", "Ore"),
        (res.crops, res.has_double_crops(), "crops", "Crops"),
        (res.fish, res.has_double_fish(), "fish", "Fish"),
        (res.wood, res.has_double_wood(), "wood", "Wood"),
    ];
    for &(val, is_double, icon, label) in items {
        if val > 0 {
            let icon_tag = format!(
                r#"<img src="/icons/{icon}.svg" style="width:11px;height:11px;vertical-align:middle;image-rendering:pixelated;" />"#
            );
            let double_marker = if is_double {
                format!(
                    r#"<img src="/icons/{icon}.svg" style="width:11px;height:11px;vertical-align:middle;image-rendering:pixelated;" />"#
                )
            } else {
                String::new()
            };
            let amount = format_resource_compact(val);
            html.push_str(&format!(
                r#"<span style="display:inline-flex;align-items:center;gap:3px;background:#1a1d2a;padding:1px 5px;border-radius:3px;border:1px solid #282c3e;">{icon_tag}{double_marker}<span style="font-size:0.6rem;color:#e2e0d8;">{amount}</span><span style="font-size:0.52rem;color:#5a5860;">{label}</span></span>"#
            ));
        }
    }
    html
}

/// Root application component. Provides global reactive signals via context.
#[component]
pub fn App() -> impl IntoView {
    // Global signals
    let territories: RwSignal<ClientTerritoryMap> = RwSignal::new(Default::default());
    let viewport: RwSignal<Viewport> = RwSignal::new(Viewport::default());
    let hovered: RwSignal<Option<String>> = RwSignal::new(None);
    let selected: RwSignal<Option<String>> = RwSignal::new(None);
    let search_query: RwSignal<String> = RwSignal::new(String::new());
    let connection: RwSignal<ConnectionStatus> = RwSignal::new(ConnectionStatus::Connecting);
    let mouse_pos: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    let loaded_tiles: RwSignal<Vec<LoadedTile>> = RwSignal::new(Vec::new());
    let loaded_icons: RwSignal<Option<ResourceIcons>> = RwSignal::new(None);
    // Epoch-second tick — drives cooldown countdown updates across canvas, tooltip, sidebar
    let tick: RwSignal<i64> = RwSignal::new(chrono::Utc::now().timestamp());
    let saved: Settings = gloo_storage::LocalStorage::get("sequoia_settings").unwrap_or_default();
    let show_connections: RwSignal<bool> = RwSignal::new(saved.show_connections);
    let abbreviate_names: RwSignal<bool> = RwSignal::new(saved.abbreviate_names);
    let show_countdown: RwSignal<bool> = RwSignal::new(saved.show_countdown);
    let show_granular_map_time: RwSignal<bool> = RwSignal::new(saved.granular_map_time);
    let show_names: RwSignal<bool> = RwSignal::new(saved.show_names);
    let thick_cooldown_borders: RwSignal<bool> = RwSignal::new(saved.thick_cooldown_borders);
    let bold_names: RwSignal<bool> = RwSignal::new(saved.bold_names);
    let bold_tags: RwSignal<bool> = RwSignal::new(saved.bold_tags);
    let thick_tag_outline: RwSignal<bool> = RwSignal::new(saved.thick_tag_outline);
    let thick_name_outline: RwSignal<bool> = RwSignal::new(saved.thick_name_outline);
    let readable_font: RwSignal<bool> = RwSignal::new(saved.readable_font);
    let bold_connections: RwSignal<bool> = RwSignal::new(saved.bold_connections);
    let resource_highlight: RwSignal<bool> = RwSignal::new(saved.resource_highlight);
    let name_color: RwSignal<NameColor> = RwSignal::new(saved.name_color);
    let sidebar_open: RwSignal<bool> = RwSignal::new(saved.sidebar_open);
    let sidebar_ready: RwSignal<bool> = RwSignal::new(false);
    let sidebar_loaded: RwSignal<bool> = RwSignal::new(saved.sidebar_open);
    let sidebar_index: RwSignal<usize> = RwSignal::new(0);
    let sidebar_items: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    // Live-first boot: defer non-essential work (tiles/minimap/history checks/icons)
    // until we have initial territory data and a short settle window.
    let deferred_boot_ready: RwSignal<bool> = RwSignal::new(false);
    let deferred_boot_timer_set: RwSignal<bool> = RwSignal::new(false);
    let tile_fetch_scheduled: RwSignal<bool> = RwSignal::new(false);
    let minimap_mount_scheduled: RwSignal<bool> = RwSignal::new(false);
    let minimap_loaded: RwSignal<bool> = RwSignal::new(false);
    let icons_loaded: RwSignal<bool> = RwSignal::new(false);
    let loading_shell_removed: RwSignal<bool> = RwSignal::new(false);

    // History mode signals
    let map_mode: RwSignal<MapMode> = RwSignal::new(MapMode::Live);
    let history_timestamp: RwSignal<Option<i64>> = RwSignal::new(None);
    let playback_active: RwSignal<bool> = RwSignal::new(false);
    let playback_speed: RwSignal<f64> = RwSignal::new(10.0);
    let history_bounds: RwSignal<Option<(i64, i64)>> = RwSignal::new(None);
    let history_available: RwSignal<bool> = RwSignal::new(false);
    let history_fetch_nonce: RwSignal<u64> = RwSignal::new(0);
    let last_live_seq: RwSignal<Option<u64>> = RwSignal::new(None);
    let history_buffered_updates: RwSignal<Vec<BufferedUpdate>> = RwSignal::new(Vec::new());
    let history_buffer_mode_active: RwSignal<bool> = RwSignal::new(false);
    let needs_live_resync: RwSignal<bool> = RwSignal::new(false);
    let live_resync_in_flight: RwSignal<bool> = RwSignal::new(false);
    let live_handoff_resync_count: RwSignal<u64> = RwSignal::new(0);
    let sse_seq_gap_detected_count: RwSignal<u64> = RwSignal::new(0);
    let history_buffer_size_max: RwSignal<usize> = RwSignal::new(0);
    let territory_geometry: StoredValue<TerritoryGeometryMap> = StoredValue::new(HashMap::new());
    let guild_colors: StoredValue<GuildColorMap> = StoredValue::new(HashMap::new());

    // Provide via context so children can access
    provide_context(territories);
    provide_context(viewport);
    provide_context(Hovered(hovered));
    provide_context(Selected(selected));
    provide_context(search_query);
    provide_context(connection);
    provide_context(mouse_pos);
    provide_context(loaded_tiles);
    provide_context(loaded_icons);
    provide_context(tick);
    provide_context(show_connections);
    provide_context(AbbreviateNames(abbreviate_names));
    provide_context(ShowCountdown(show_countdown));
    provide_context(ShowGranularMapTime(show_granular_map_time));
    provide_context(ShowNames(show_names));
    provide_context(ThickCooldownBorders(thick_cooldown_borders));
    provide_context(BoldNames(bold_names));
    provide_context(BoldTags(bold_tags));
    provide_context(ThickTagOutline(thick_tag_outline));
    provide_context(ThickNameOutline(thick_name_outline));
    provide_context(ReadableFont(readable_font));
    provide_context(BoldConnections(bold_connections));
    provide_context(ResourceHighlight(resource_highlight));
    provide_context(NameColorSetting(name_color));
    provide_context(SidebarOpen(sidebar_open));
    provide_context(SidebarIndex(sidebar_index));
    provide_context(SidebarItems(sidebar_items));
    provide_context(CurrentMode(map_mode));
    provide_context(HistoryTimestamp(history_timestamp));
    provide_context(PlaybackActive(playback_active));
    provide_context(PlaybackSpeed(playback_speed));
    provide_context(HistoryBoundsSignal(history_bounds));
    provide_context(HistoryAvailable(history_available));
    provide_context(HistoryFetchNonce(history_fetch_nonce));
    provide_context(LastLiveSeq(last_live_seq));
    provide_context(HistoryBufferedUpdates(history_buffered_updates));
    provide_context(HistoryBufferModeActive(history_buffer_mode_active));
    provide_context(NeedsLiveResync(needs_live_resync));
    provide_context(LiveResyncInFlight(live_resync_in_flight));
    provide_context(LiveHandoffResyncCount(live_handoff_resync_count));
    provide_context(SseSeqGapDetectedCount(sse_seq_gap_detected_count));
    provide_context(HistoryBufferSizeMax(history_buffer_size_max));
    provide_context(TerritoryGeometryStore(territory_geometry));
    provide_context(GuildColorStore(guild_colors));
    provide_context(crate::tower::TowerState::new());

    // Persist settings to localStorage on any change
    Effect::new(move || {
        let settings = Settings {
            show_connections: show_connections.get(),
            abbreviate_names: abbreviate_names.get(),
            show_countdown: show_countdown.get(),
            granular_map_time: show_granular_map_time.get(),
            show_names: show_names.get(),
            thick_cooldown_borders: thick_cooldown_borders.get(),
            bold_names: bold_names.get(),
            bold_tags: bold_tags.get(),
            thick_tag_outline: thick_tag_outline.get(),
            thick_name_outline: thick_name_outline.get(),
            readable_font: readable_font.get(),
            bold_connections: bold_connections.get(),
            name_color: name_color.get(),
            sidebar_open: sidebar_open.get(),
            resource_highlight: resource_highlight.get(),
        };
        let _ = gloo_storage::LocalStorage::set("sequoia_settings", &settings);
    });

    // Enable sidebar transitions only after initial mount to avoid first-paint animation flash.
    Effect::new(move || {
        sidebar_ready.set(true);
    });

    // Lazy-mount sidebar panel on first open to keep initial boot focused on live map rendering.
    Effect::new(move || {
        if sidebar_open.get() && !sidebar_loaded.get_untracked() {
            sidebar_loaded.set(true);
        }
    });

    // 1-second interval to advance the tick signal (triggers cooldown re-renders)
    Effect::new({
        move || {
            use wasm_bindgen::prelude::*;
            let Some(window) = web_sys::window() else {
                return;
            };

            TICK_INTERVAL_BINDING.with(|slot| {
                if let Some(old) = slot.borrow_mut().take() {
                    old.window.clear_interval_with_handle(old.interval_id);
                }
            });

            let cb = Closure::<dyn Fn()>::new(move || {
                tick.set(chrono::Utc::now().timestamp());
            });
            let Ok(interval_id) = window.set_interval_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                1_000,
            ) else {
                return;
            };
            TICK_INTERVAL_BINDING.with(|slot| {
                *slot.borrow_mut() = Some(TickIntervalBinding {
                    window: window.clone(),
                    interval_id,
                    _callback: cb,
                });
            });
        }
    });

    // Connect to SSE on mount
    Effect::new(move || {
        sse::connect(territories, connection);
        on_cleanup(|| {
            sse::disconnect();
        });
    });

    // Once initial territory data arrives, defer non-essential boot tasks slightly.
    Effect::new(move || {
        let has_territories = !territories.get().is_empty();
        if !has_territories
            || deferred_boot_ready.get_untracked()
            || deferred_boot_timer_set.get_untracked()
        {
            return;
        }

        deferred_boot_timer_set.set(true);
        if let Some(window) = web_sys::window() {
            let cb = wasm_bindgen::closure::Closure::once(move || {
                deferred_boot_ready.set(true);
            });
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                800,
            );
            cb.forget();
        } else {
            deferred_boot_ready.set(true);
        }
    });

    // Keep shell step text tied to real startup milestones.
    Effect::new(move || {
        let has_territories = !territories.get().is_empty();
        if deferred_boot_ready.get() {
            set_loading_shell_step("Starting renderer");
        } else if has_territories {
            set_loading_shell_step("Syncing territory data");
        } else {
            set_loading_shell_step("Connecting to API");
        }
    });

    // Remove static shell shortly after boot is ready so the final step is visible briefly.
    Effect::new(move || {
        if !deferred_boot_ready.get() || loading_shell_removed.get_untracked() {
            return;
        }
        loading_shell_removed.set(true);
        if let Some(window) = web_sys::window() {
            let cb = wasm_bindgen::closure::Closure::once(|| {
                remove_loading_shell();
            });
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                240,
            );
            cb.forget();
        } else {
            remove_loading_shell();
        }
    });

    // Schedule background tile loading only after first live paint and idle time.
    Effect::new(move || {
        if !deferred_boot_ready.get() || tile_fetch_scheduled.get_untracked() {
            return;
        }
        tile_fetch_scheduled.set(true);

        let Some(window) = web_sys::window() else {
            tiles::fetch_tiles(loaded_tiles);
            return;
        };

        let callback = wasm_bindgen::closure::Closure::once(move || {
            tiles::fetch_tiles(loaded_tiles);
        });
        let mut scheduled = false;
        if let Ok(idle_fn) =
            Reflect::get(window.as_ref(), &JsValue::from_str("requestIdleCallback"))
            && let Ok(idle_fn) = idle_fn.dyn_into::<Function>()
        {
            let _ = idle_fn.call1(window.as_ref(), callback.as_ref().unchecked_ref());
            scheduled = true;
        }
        if !scheduled {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                4_000,
            );
        }
        callback.forget();
    });

    // Defer minimap mount until idle time so startup stays focused on territory rendering.
    Effect::new(move || {
        if !deferred_boot_ready.get() || minimap_mount_scheduled.get_untracked() {
            return;
        }
        minimap_mount_scheduled.set(true);

        let Some(window) = web_sys::window() else {
            minimap_loaded.set(true);
            return;
        };

        let callback = wasm_bindgen::closure::Closure::once(move || {
            minimap_loaded.set(true);
        });
        let mut scheduled = false;
        if let Ok(idle_fn) =
            Reflect::get(window.as_ref(), &JsValue::from_str("requestIdleCallback"))
            && let Ok(idle_fn) = idle_fn.dyn_into::<Function>()
        {
            let _ = idle_fn.call1(window.as_ref(), callback.as_ref().unchecked_ref());
            scheduled = true;
        }
        if !scheduled {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                2_000,
            );
        }
        callback.forget();
    });

    // Lazy-load icon atlas only when resource highlights are actually used.
    Effect::new(move || {
        if !deferred_boot_ready.get() || !resource_highlight.get() || icons_loaded.get_untracked() {
            return;
        }
        icons_loaded.set(true);
        icons::load_resource_icons(loaded_icons);
    });

    // Start playback engine (runs continuously, only active when playing)
    Effect::new(move || {
        crate::playback::start_playback_engine();
    });

    // Global keyboard shortcuts
    Effect::new(move || {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::prelude::*;

        let Some(window) = web_sys::window() else {
            return;
        };

        KEYDOWN_BINDING.with(|slot| {
            if let Some(old) = slot.borrow_mut().take() {
                let _ = old.window.remove_event_listener_with_callback(
                    "keydown",
                    old._handler.as_ref().unchecked_ref(),
                );
            }
        });

        let handler =
            Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
                let key = e.key();
                let target_tag = e
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                    .map(|el| el.tag_name())
                    .unwrap_or_default();

                // Don't intercept when typing in an input
                if target_tag == "INPUT" || target_tag == "TEXTAREA" {
                    if key == "Escape"
                        && let Some(el) = e
                            .target()
                            .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                    {
                        el.blur().ok();
                    }
                    return;
                }

                match key.as_str() {
                    "Escape" => {
                        selected.set(None);
                        hovered.set(None);
                    }
                    "/" => {
                        e.prevent_default();
                        let Some(window) = web_sys::window() else {
                            return;
                        };
                        let Some(doc) = window.document() else {
                            return;
                        };
                        if let Some(el) = doc.query_selector("[data-search-input]").ok().flatten()
                            && let Ok(input) = el.dyn_into::<web_sys::HtmlElement>()
                        {
                            input.focus().ok();
                        }
                    }
                    "a" => {
                        abbreviate_names.update(|v| *v = !*v);
                    }
                    "n" => {
                        show_names.update(|v| *v = !*v);
                    }
                    "t" => {
                        show_countdown.update(|v| *v = !*v);
                    }
                    "c" => {
                        show_connections.update(|v| *v = !*v);
                    }
                    "b" => {
                        bold_connections.update(|v| *v = !*v);
                    }
                    "p" => {
                        resource_highlight.update(|v| *v = !*v);
                    }
                    "h" => {
                        let mode = map_mode.get_untracked();
                        match mode {
                            MapMode::Live => {
                                if !history_available.get_untracked() {
                                    history::check_availability(history_available);
                                    return;
                                }
                                if history_available.get_untracked() {
                                    history::enter_history_mode(history::EnterHistoryModeInput {
                                        mode: map_mode,
                                        history_timestamp,
                                        history_bounds,
                                        history_fetch_nonce,
                                        history_buffered_updates,
                                        history_buffer_mode_active,
                                        needs_live_resync,
                                        geo_store: territory_geometry,
                                        guild_color_store: guild_colors,
                                        territories,
                                    });
                                }
                            }
                            MapMode::History => {
                                history::exit_history_mode(history::ExitHistoryModeInput {
                                    mode: map_mode,
                                    playback_active,
                                    history_fetch_nonce,
                                    history_timestamp,
                                    history_buffered_updates,
                                    history_buffer_mode_active,
                                    last_live_seq,
                                    needs_live_resync,
                                    live_handoff_resync_count,
                                    territories,
                                });
                            }
                        }
                    }
                    " " => {
                        if map_mode.get_untracked() == MapMode::History {
                            e.prevent_default();
                            playback_active.update(|v| *v = !*v);
                        }
                    }
                    "[" => {
                        if map_mode.get_untracked() == MapMode::History {
                            history::step_backward(
                                history_timestamp,
                                playback_active,
                                map_mode,
                                history_fetch_nonce,
                                territory_geometry,
                                guild_colors,
                                territories,
                            );
                        }
                    }
                    "]" => {
                        if map_mode.get_untracked() == MapMode::History {
                            history::step_forward(
                                history_timestamp,
                                playback_active,
                                map_mode,
                                history_fetch_nonce,
                                territory_geometry,
                                guild_colors,
                                territories,
                            );
                        }
                    }
                    "r" | "0" => {
                        viewport.update(|vp| {
                            let territories = territories.get_untracked();
                            if territories.is_empty() {
                                return;
                            }
                            let (mut min_x, mut min_y, mut max_x, mut max_y) =
                                (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                            for ct in territories.values() {
                                let loc = &ct.territory.location;
                                min_x = min_x.min(loc.left() as f64);
                                min_y = min_y.min(loc.top() as f64);
                                max_x = max_x.max(loc.right() as f64);
                                max_y = max_y.max(loc.bottom() as f64);
                            }
                            let (cw, ch) = canvas_dimensions();
                            vp.fit_bounds(min_x, min_y, max_x, max_y, cw, ch);
                        });
                    }
                    "j" | "ArrowDown" => {
                        e.prevent_default();
                        let items = sidebar_items.get_untracked();
                        if !items.is_empty() {
                            sidebar_index.update(|i| *i = (*i + 1).min(items.len() - 1));
                        }
                    }
                    "k" | "ArrowUp" => {
                        e.prevent_default();
                        let items = sidebar_items.get_untracked();
                        if !items.is_empty() {
                            sidebar_index.update(|i| *i = i.saturating_sub(1));
                        }
                    }
                    "Enter" => {
                        let items = sidebar_items.get_untracked();
                        let idx = sidebar_index.get_untracked();
                        if let Some(name) = items.get(idx) {
                            selected.set(Some(name.clone()));
                            if map_mode.get_untracked() == MapMode::Live
                                && !sidebar_open.get_untracked()
                            {
                                sidebar_open.set(true);
                            }
                            let map = territories.get_untracked();
                            if let Some(ct) = map.get(name) {
                                let loc = &ct.territory.location;
                                let (cw, ch) = canvas_dimensions();
                                viewport.update(|vp| {
                                    vp.fit_bounds(
                                        loc.left() as f64 - 200.0,
                                        loc.top() as f64 - 200.0,
                                        loc.right() as f64 + 200.0,
                                        loc.bottom() as f64 + 200.0,
                                        cw,
                                        ch,
                                    );
                                });
                            }
                        }
                    }
                    "ArrowLeft" => {
                        e.prevent_default();
                        viewport.update(|vp| vp.pan(50.0 / vp.scale, 0.0));
                    }
                    "ArrowRight" => {
                        e.prevent_default();
                        viewport.update(|vp| vp.pan(-50.0 / vp.scale, 0.0));
                    }
                    "+" | "=" => {
                        e.prevent_default();
                        let (cw, ch) = canvas_dimensions();
                        viewport.update(|vp| vp.zoom_at(-120.0, cw / 2.0, ch / 2.0));
                    }
                    "-" => {
                        e.prevent_default();
                        let (cw, ch) = canvas_dimensions();
                        viewport.update(|vp| vp.zoom_at(120.0, cw / 2.0, ch / 2.0));
                    }
                    _ => {}
                }
            });

        if window
            .add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref())
            .is_ok()
        {
            KEYDOWN_BINDING.with(|slot| {
                *slot.borrow_mut() = Some(KeydownBinding {
                    window: window.clone(),
                    _handler: handler,
                });
            });
        }
    });

    view! {
        <div style="width: 100%; height: 100%; position: relative;">
            <div style="width: 100%; height: 100%; position: relative; overflow: hidden; background: #0c0e17;">
                <MapCanvas />
                {move || {
                    if minimap_loaded.get() {
                        view! { <Minimap /> }.into_any()
                    } else {
                        ().into_any()
                    }
                }}
            </div>
            <div
                class="sidebar-wrapper"
                class:sidebar-ready=move || sidebar_ready.get()
                style:transform=move || if sidebar_open.get() { "translateX(0)" } else { "translateX(100%)" }
                style:pointer-events=move || if sidebar_open.get() { "auto" } else { "none" }
            >
                <SidebarToggle />
                {move || {
                    if sidebar_loaded.get() {
                        view! { <Sidebar /> }.into_any()
                    } else {
                        ().into_any()
                    }
                }}
            </div>
            {move || {
                if map_mode.get() == MapMode::History {
                    view! { <Timeline /> }.into_any()
                } else {
                    ().into_any()
                }
            }}
        </div>
        <Tooltip />
    }
}

/// Toggle button for showing/hiding the sidebar. Attached to the sidebar's left edge.
#[component]
fn SidebarToggle() -> impl IntoView {
    let SidebarOpen(sidebar_open) = expect_context();

    view! {
        <button
            class="sidebar-toggle"
            title=move || if sidebar_open.get() { "Hide sidebar" } else { "Show sidebar" }
            style="position: absolute; top: 16px; left: -44px; z-index: 11; width: 32px; height: 32px; background: #13161f; border: 1px solid #282c3e; border-radius: 6px; cursor: pointer; display: flex; align-items: center; justify-content: center; transition: border-color 0.15s, background 0.15s, color 0.15s; color: #5a5860; font-family: 'JetBrains Mono', monospace; font-size: 1.1rem; line-height: 1;"
            on:click=move |_| sidebar_open.update(|v| *v = !*v)
            on:mouseenter=move |e| {
                if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                    el.style().set_property("border-color", "rgba(245,197,66,0.4)").ok();
                    el.style().set_property("color", "#f5c542").ok();
                    el.style().set_property("background", "#1a1d2a").ok();
                }
            }
            on:mouseleave=move |e| {
                if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                    el.style().set_property("border-color", "#282c3e").ok();
                    el.style().set_property("color", "#5a5860").ok();
                    el.style().set_property("background", "#13161f").ok();
                }
            }
        >
            {move || if sidebar_open.get() { "\u{00BB}" } else { "\u{00AB}" }}
        </button>
    }
}

/// Tooltip that follows the mouse cursor when hovering a territory.
#[component]
fn Tooltip() -> impl IntoView {
    let Hovered(hovered) = expect_context();
    let territories: RwSignal<ClientTerritoryMap> = expect_context();
    let mouse_pos: RwSignal<(f64, f64)> = expect_context();
    let tick: RwSignal<i64> = expect_context();
    let CurrentMode(mode) = expect_context();
    let HistoryTimestamp(history_timestamp) = expect_context();
    let AbbreviateNames(abbreviate_names) = expect_context();

    let tooltip_info = Memo::new(move |_| {
        let reference_secs = if mode.get() == MapMode::History {
            history_timestamp.get().unwrap_or_else(|| tick.get())
        } else {
            tick.get()
        };
        let name = hovered.get()?;
        let map = territories.get();
        let ct = map.get(&name)?;
        let (r, g, b) = ct.guild_color;
        let resources = ct.territory.resources.clone();
        let acquired = ct.territory.acquired.to_rfc3339();
        let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&acquired) else {
            let treasury = TreasuryLevel::VeryLow;
            return Some((
                name,
                ct.territory.guild.name.clone(),
                ct.territory.guild.prefix.clone(),
                format_hms(0),
                (r, g, b),
                None::<(String, f64)>,
                treasury,
                resources,
            ));
        };
        let secs = (reference_secs - dt.timestamp()).max(0);
        let held = format_hms(secs);
        let cooldown = if secs < 600 {
            let remaining = 600 - secs;
            let frac = remaining as f64 / 600.0;
            Some((format!("{}:{:02}", remaining / 60, remaining % 60), frac))
        } else {
            None
        };
        let treasury = TreasuryLevel::from_held_seconds(secs);
        Some((
            name,
            ct.territory.guild.name.clone(),
            ct.territory.guild.prefix.clone(),
            held,
            (r, g, b),
            cooldown,
            treasury,
            resources,
        ))
    });

    view! {
        {move || {
            let Some(info) = tooltip_info.get() else {
                return view! { <div style="display:none;" /> }.into_any();
            };
            let (x, y) = mouse_pos.get();
            let (r, g, b) = info.4;
            let cooldown = info.5;
            let treasury = info.6;
            let resources = info.7;
            let (tr, tg, tb) = treasury.color_rgb();
            let buff = treasury.buff_percent();
            view! {
                <div
                    class="tooltip-animate"
                    style:left=format!("{}px", x + 16.0)
                    style:top=format!("{}px", y - 8.0)
                    style="position: fixed; pointer-events: none; z-index: 100; background: #161921; border: 1px solid #282c3e; border-radius: 6px; overflow: hidden; box-shadow: 0 4px 16px rgba(0,0,0,0.5); max-width: 220px; display: flex; flex-direction: row;"
                >
                    <div style={format!("width: 3px; flex-shrink: 0; background: {};", rgba_css(r, g, b, 0.85))} />
                    <div style="padding: 8px 10px; flex: 1;">
                        <div style="font-size: 0.82rem; font-weight: 700; color: #e2e0d8; font-family: 'Silkscreen', monospace; line-height: 1.3;">
                            <span style="color: #9a9590; font-weight: 400;">"[" {info.2.clone()} "] "</span>
                            {info.1}
                        </div>
                        <div style="font-size: 0.72rem; color: #9a9590; font-family: 'JetBrains Mono', monospace; margin-top: 2px;">
                            {if abbreviate_names.get() { abbreviate_name(&info.0) } else { info.0.clone() }}
                        </div>
                        <div style="font-size: 0.65rem; margin-top: 5px; padding-top: 4px; border-top: 1px solid rgba(40,44,62,0.5); display: flex; justify-content: space-between; align-items: center; gap: 8px;">
                            <span style="color: #9a9590; font-family: 'Inter', system-ui, sans-serif;">"Held"</span>
                            <span style="color: #e2e0d8; font-family: 'JetBrains Mono', monospace; font-variant-numeric: tabular-nums;">{info.3}</span>
                        </div>
                        <div style="font-size: 0.65rem; font-family: 'JetBrains Mono', monospace; margin-top: 3px; padding-top: 3px; border-top: 1px solid rgba(40,44,62,0.5); display: flex; align-items: center; gap: 4px;">
                            <span style={format!("color: {}; font-size: 0.5rem;", rgba_css(tr, tg, tb, 1.0))}>{"\u{25C6}"}</span>
                            <span style={format!("color: {};", rgba_css(tr, tg, tb, 0.9))}>{treasury.label()}</span>
                            {(buff > 0).then(|| view! {
                                <span style="color: #5a5860; margin-left: auto; font-size: 0.58rem;">{format!("+{}%", buff)}</span>
                            })}
                        </div>
                        {(!resources.is_empty()).then(|| {
                            let icons_html = resource_icons_html(&resources);
                            view! {
                                <div style="font-size: 0.65rem; font-family: 'JetBrains Mono', monospace; margin-top: 3px; padding-top: 3px; border-top: 1px solid rgba(40,44,62,0.5); display: flex; flex-wrap: wrap; align-items: center; gap: 3px;">
                                    <span style="display: contents;" inner_html=icons_html />
                                </div>
                            }
                        })}
                        {cooldown.map(|(remaining, frac)| view! {
                            <div style="margin-top: 4px; padding-top: 3px; border-top: 1px solid rgba(40,44,62,0.5);">
                                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 3px;">
                                    <span style="font-size: 0.62rem; color: #f5c542; font-family: 'Inter', system-ui, sans-serif;">"Cooldown"</span>
                                    <span style="font-size: 0.65rem; color: #f5c542; font-family: 'JetBrains Mono', monospace;">{remaining}</span>
                                </div>
                                <div style="height: 3px; background: rgba(255,255,255,0.06); border-radius: 2px; overflow: hidden;">
                                    <div style={format!(
                                        "height: 100%; width: {:.1}%; background: linear-gradient(to right, #f5c542, #d4a030); border-radius: 2px;",
                                        frac * 100.0
                                    )} />
                                </div>
                            </div>
                        })}
                    </div>
                </div>
            }.into_any()
        }}
    }
}
