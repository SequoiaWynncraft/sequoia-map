use std::collections::HashMap;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use sequoia_shared::{Resources, TreasuryLevel};

use crate::app::{
    AbbreviateNames, BoldConnections, BoldNames, BoldTags, CurrentMode, GuildColorStore,
    HistoryAvailable, HistoryBoundsSignal, HistoryBufferModeActive, HistoryBufferedUpdates,
    HistoryFetchNonce, HistoryTimestamp, LastLiveSeq, LiveHandoffResyncCount, MapMode, NameColor,
    NameColorSetting, NeedsLiveResync, PlaybackActive, ReadableFont, ResourceHighlight,
    SIDEBAR_WIDTH, Selected, ShowCountdown, ShowGranularMapTime, ShowNames, SidebarIndex,
    SidebarItems, SidebarOpen, TerritoryGeometryStore, ThickCooldownBorders, ThickNameOutline,
    ThickTagOutline, canvas_dimensions,
};
use crate::colors::rgba_css;
use crate::history;
use crate::sse::ConnectionStatus;
use crate::territory::ClientTerritoryMap;
use crate::tower::TowerCalculator;
use crate::viewport::Viewport;

/// Build list of (label, formatted_value, icon_name) for non-zero resources.
fn build_resource_items(res: &Resources) -> Vec<(&'static str, String, &'static str)> {
    let mut items = Vec::new();
    if res.emeralds > 0 {
        items.push(("Emeralds", format_resource(res.emeralds), "emerald"));
    }
    if res.ore > 0 {
        items.push(("Ore", format_resource(res.ore), "ore"));
    }
    if res.crops > 0 {
        items.push(("Crops", format_resource(res.crops), "crops"));
    }
    if res.fish > 0 {
        items.push(("Fish", format_resource(res.fish), "fish"));
    }
    if res.wood > 0 {
        items.push(("Wood", format_resource(res.wood), "wood"));
    }
    items
}

fn format_resource(val: i32) -> String {
    if val >= 1000 {
        format!("{:.1}k", val as f64 / 1000.0)
    } else {
        format!("{}", val)
    }
}

/// Format an RFC3339 timestamp into a human-readable relative time.
fn format_relative_time(rfc3339: &str, reference_secs: i64) -> String {
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rfc3339) else {
        return rfc3339.to_string();
    };
    let secs = reference_secs - dt.timestamp();
    if secs <= 0 {
        return "just now".to_string();
    }
    if secs < 60 {
        return "just now".to_string();
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{}m ago", mins);
    }
    let hours = secs / 3600;
    if hours < 24 {
        return format!("{}h ago", hours);
    }
    let days = secs / 86_400;
    if days < 7 {
        return format!("{}d ago", days);
    }
    if days < 30 {
        let weeks = days / 7;
        return format!("{}w ago", weeks);
    }
    // Fallback to short date
    dt.format("%b %d, %Y").to_string()
}

#[derive(Clone, Copy)]
struct ShowSettings(RwSignal<bool>);

/// Sidebar with search, leaderboard, detail panel, and stats.
#[component]
pub fn Sidebar() -> impl IntoView {
    let search_query: RwSignal<String> = expect_context();
    let Selected(selected) = expect_context();
    let SidebarOpen(sidebar_open) = expect_context();
    let SidebarIndex(sidebar_index) = expect_context();
    let SidebarItems(sidebar_items) = expect_context();
    let show_settings = RwSignal::new(false);
    provide_context(ShowSettings(show_settings));

    // Scroll focused item into view when index changes
    Effect::new(move || {
        let idx = sidebar_index.get();
        let Some(window) = web_sys::window() else {
            return;
        };
        let Some(doc) = window.document() else {
            return;
        };
        if let Ok(Some(el)) = doc.query_selector(&format!("[data-sidebar-idx='{}']", idx)) {
            let opts = web_sys::ScrollIntoViewOptions::new();
            opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
            el.scroll_into_view_with_scroll_into_view_options(&opts);
        }
    });

    view! {
        <div
            class:sidebar-animate=move || sidebar_open.get()
            style:display=move || if sidebar_open.get() { "flex" } else { "none" }
            style=format!("width: {}px; min-width: {}px; height: 100%; background: #13161f; border-left: 1px solid #282c3e; display: flex; flex-direction: column; z-index: 10; box-shadow: -4px 0 20px rgba(0,0,0,0.4), inset 1px 0 0 rgba(168,85,247,0.04);", SIDEBAR_WIDTH as u32, SIDEBAR_WIDTH as u32)
        >
            <SidebarHeader />
            <SearchBar />
            <div class="scrollbar-thin" style="flex: 1; overflow-y: auto;">
                {move || {
                    if show_settings.get() {
                        sidebar_items.set(Vec::new());
                        sidebar_index.set(0);
                        view! { <SettingsPanel /> }.into_any()
                    } else if selected.get().is_some() {
                        sidebar_items.set(Vec::new());
                        sidebar_index.set(0);
                        view! { <DetailPanel /> }.into_any()
                    } else {
                        let query = search_query.get();
                        if query.is_empty() {
                            view! { <LeaderboardPanel /> }.into_any()
                        } else {
                            view! { <SearchResults /> }.into_any()
                        }
                    }
                }}
            </div>
            <StatsBar />
        </div>
    }
}

#[component]
fn SidebarHeader() -> impl IntoView {
    view! {
        <div style="padding: 20px 24px 16px; border-bottom: 1px solid #282c3e;">
            <div style="display: flex; align-items: baseline; gap: 10px;">
                <div class="text-gold-gradient" style="font-family: 'Silkscreen', monospace; font-size: 1.25rem; font-weight: 700; letter-spacing: 0.18em; text-transform: uppercase; text-shadow: 0 0 16px rgba(245,197,66,0.08);">"SEQUOIA"</div>
                <div style="font-family: 'JetBrains Mono', monospace; font-size: 0.58rem; color: #3a3f5c; background: #1a1d2a; padding: 1px 6px; border-radius: 3px; border: 1px solid rgba(245,197,66,0.15); letter-spacing: 0.04em;">"v0.1"</div>
            </div>
            <div style="font-family: 'Inter', system-ui, sans-serif; font-size: 0.72rem; color: #5a5860; margin-top: 3px; letter-spacing: 0.08em;">"Wynncraft Territories"</div>
            // Gradient line divider
            <div class="divider-gold" style="margin-top: 12px;" />
        </div>
    }
}

#[component]
fn SearchBar() -> impl IntoView {
    let search_query: RwSignal<String> = expect_context();

    let on_input = move |e: leptos::ev::Event| {
        let Some(target) = e.target() else {
            return;
        };
        let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() else {
            return;
        };
        search_query.set(input.value());
    };

    view! {
        <div style="padding: 12px 24px; border-bottom: 1px solid #282c3e;">
            <div style="position: relative;">
                // Search icon (inline SVG magnifying glass)
                <div style="position: absolute; left: 12px; top: 50%; transform: translateY(-50%); pointer-events: none; color: #5a5860; width: 14px; height: 14px;">
                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" width="14" height="14">
                        <path fill-rule="evenodd" d="M9 3.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM2 9a7 7 0 1112.452 4.391l3.328 3.329a.75.75 0 11-1.06 1.06l-3.329-3.328A7 7 0 012 9z" clip-rule="evenodd" />
                    </svg>
                </div>
                <input
                    data-search-input=""
                    class="focus-ring"
                    style="width: 100%; padding: 10px 14px 10px 34px; background: #1a1d2a; border: 1px solid #282c3e; border-radius: 6px; color: #e2e0d8; font-family: 'Inter', system-ui, sans-serif; font-size: 0.9rem; outline: none; transition: border-color 0.2s ease, box-shadow 0.3s ease;"
                    type="text"
                    placeholder="Search territories or guilds..."
                    prop:value=move || search_query.get()
                    on:input=on_input
                    on:focus=|e| {
                        if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                            el.style().set_property("border-color", "#f5c542").ok();
                            el.style().set_property("box-shadow", "0 0 12px rgba(245,197,66,0.08)").ok();
                        }
                    }
                    on:blur=|e| {
                        if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                            el.style().set_property("border-color", "#282c3e").ok();
                            el.style().set_property("box-shadow", "none").ok();
                        }
                    }
                />
                // Keyboard hint
                <div style="position: absolute; right: 10px; top: 50%; transform: translateY(-50%); font-family: 'JetBrains Mono', monospace; font-size: 0.62rem; color: #3a3f5c; background: #13161f; padding: 1px 5px; border-radius: 3px; border: 1px solid #282c3e; pointer-events: none;">"/"</div>
            </div>
        </div>
    }
}

#[component]
fn SettingsPanel() -> impl IntoView {
    let AbbreviateNames(abbreviate_names) = expect_context();
    let show_connections: RwSignal<bool> = expect_context();
    let ShowCountdown(show_countdown) = expect_context();
    let ShowGranularMapTime(show_granular_map_time) = expect_context();
    let ShowNames(show_names) = expect_context();
    let ThickCooldownBorders(thick_cooldown_borders) = expect_context();
    let BoldNames(bold_names) = expect_context();
    let BoldTags(bold_tags) = expect_context();
    let ThickTagOutline(thick_tag_outline) = expect_context();
    let ThickNameOutline(thick_name_outline) = expect_context();
    let ReadableFont(readable_font) = expect_context();
    let BoldConnections(bold_connections) = expect_context();
    let ResourceHighlight(resource_highlight) = expect_context();

    view! {
        <div style="border-bottom: 1px solid #282c3e;">
            <div style="padding: 14px 24px 8px; font-family: 'Silkscreen', monospace; font-size: 0.85rem; text-transform: uppercase; letter-spacing: 0.14em; color: #5a5860;">
                <span style="color: #f5c542; margin-right: 6px; font-size: 0.7rem;">{"\u{2699}"}</span>"Settings"
            </div>
            <div style="padding: 0 12px 12px;">
                <SettingsToggleRow label="Territory Names" shortcut="N" active=show_names />
                <SettingsToggleRow label="Abbreviate Names" shortcut="A" active=abbreviate_names />
                <SettingsToggleRow label="Bold Names" shortcut="" active=bold_names />
                <SettingsToggleRow label="Bold Guild Tags" shortcut="" active=bold_tags />
                <SettingsToggleRow label="Thick Name Outline" shortcut="" active=thick_name_outline />
                <SettingsToggleRow label="Thick Tag Outline" shortcut="" active=thick_tag_outline />
                <SettingsToggleRow label="Connection Lines" shortcut="C" active=show_connections />
                <SettingsToggleRow label="Bold Connections" shortcut="B" active=bold_connections />
                <SettingsToggleRow label="Countdown Timer" shortcut="T" active=show_countdown />
                <SettingsToggleRow label="Granular Map Time" shortcut="" active=show_granular_map_time />
                <SettingsToggleRow label="Thick Cooldown Borders" shortcut="" active=thick_cooldown_borders />
                <SettingsToggleRow label="Readable Font" shortcut="" active=readable_font />
                <SettingsToggleRow label="Resource Highlight" shortcut="P" active=resource_highlight />
                <SettingsColorRow />
            </div>
        </div>
    }
}

#[component]
fn SettingsColorRow() -> impl IntoView {
    let NameColorSetting(name_color) = expect_context();

    let swatches: [(NameColor, &str, &str); 5] = [
        (NameColor::White, "#dcdad2", "White"),
        (
            NameColor::Guild,
            "conic-gradient(#e06060, #e0c060, #60c878, #5b9bd5, #a070d0, #e06060)",
            "Guild",
        ),
        (NameColor::Gold, "#f5c542", "Gold"),
        (NameColor::Copper, "#b56727", "Copper"),
        (NameColor::Muted, "#787470", "Muted"),
    ];

    view! {
        <div
            style="display: flex; align-items: center; justify-content: space-between; padding: 9px 10px; border-radius: 4px;"
        >
            <span style="font-size: 0.88rem; color: #e2e0d8; font-family: 'Inter', system-ui, sans-serif;">"Name Color"</span>
            <div style="display: flex; align-items: center; gap: 6px;">
                {swatches.into_iter().map(|(variant, color, title)| {
                    let on_click = move |_| name_color.set(variant);
                    view! {
                        <div
                            title=title
                            style=move || {
                                let active = name_color.get() == variant;
                                format!(
                                    "width: 18px; height: 18px; border-radius: 50%; background: {}; cursor: pointer; border: 2px solid {}; transition: border-color 0.15s, box-shadow 0.15s; flex-shrink: 0;{}",
                                    color,
                                    if active { "#f5c542" } else { "transparent" },
                                    if active { " box-shadow: 0 0 6px rgba(245,197,66,0.3);" } else { "" },
                                )
                            }
                            on:click=on_click
                        />
                    }
                }).collect::<Vec<_>>()}
            </div>
        </div>
    }
}

#[component]
fn SettingsToggleRow(
    label: &'static str,
    shortcut: &'static str,
    active: RwSignal<bool>,
) -> impl IntoView {
    let on_click = move |_| {
        active.update(|v| *v = !*v);
    };

    view! {
        <div
            style="display: flex; align-items: center; justify-content: space-between; padding: 9px 10px; border-radius: 4px; cursor: pointer; transition: background 0.15s;"
            on:click=on_click
            on:mouseenter=|e| {
                if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                    el.style().set_property("background", "#232738").ok();
                }
            }
            on:mouseleave=|e| {
                if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                    el.style().set_property("background", "transparent").ok();
                }
            }
        >
            <div style="display: flex; align-items: center; gap: 8px;">
                <span style="font-size: 0.88rem; color: #e2e0d8; font-family: 'Inter', system-ui, sans-serif;">{label}</span>
                {(!shortcut.is_empty()).then(|| view! {
                    <span style="font-family: 'JetBrains Mono', monospace; font-size: 0.58rem; color: #3a3f5c; background: #1a1d2a; padding: 1px 5px; border-radius: 3px; border: 1px solid #282c3e;">{shortcut}</span>
                })}
            </div>
            <span style=move || {
                if active.get() {
                    "display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: #50c878; box-shadow: 0 0 5px rgba(80,200,120,0.4); flex-shrink: 0;"
                } else {
                    "display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: #3a3f5c; flex-shrink: 0;"
                }
            } />
        </div>
    }
}

#[component]
fn SearchResults() -> impl IntoView {
    let search_query: RwSignal<String> = expect_context();
    let territories: RwSignal<ClientTerritoryMap> = expect_context();
    let Selected(selected) = expect_context();
    let viewport: RwSignal<Viewport> = expect_context();
    let SidebarIndex(sidebar_index) = expect_context();
    let SidebarItems(sidebar_items) = expect_context();

    let filtered = Memo::new(move |_| {
        let query = search_query.get().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }

        let map = territories.get();
        let mut results: Vec<_> = map
            .iter()
            .filter(|(name, ct)| {
                name.to_lowercase().contains(&query)
                    || ct.territory.guild.name.to_lowercase().contains(&query)
                    || ct.territory.guild.prefix.to_lowercase().contains(&query)
            })
            .map(|(name, ct)| {
                (
                    name.clone(),
                    ct.territory.guild.name.clone(),
                    ct.territory.guild.prefix.clone(),
                    ct.guild_color,
                )
            })
            .collect();

        results.sort_by(|a, b| a.0.cmp(&b.0));
        results.truncate(50);
        results
    });

    // Sync sidebar items for keyboard navigation
    Effect::new(move || {
        let f = filtered.get();
        let items: Vec<String> = f.iter().map(|(name, _, _, _)| name.clone()).collect();
        let prev = sidebar_items.get_untracked();
        if items != prev {
            sidebar_index.set(0);
        }
        sidebar_items.set(items);
    });

    let result_count = Memo::new(move |_| filtered.get().len());

    view! {
        <div style="border-bottom: 1px solid #282c3e;">
            <div style="padding: 14px 24px 8px; display: flex; align-items: baseline; justify-content: space-between;">
                <span style="font-family: 'Silkscreen', monospace; font-size: 0.85rem; text-transform: uppercase; letter-spacing: 0.14em; color: #5a5860;">"Search Results"</span>
                <span style="font-family: 'JetBrains Mono', monospace; font-size: 0.65rem; color: #3a3f5c;">{move || format!("{} found", result_count.get())}</span>
            </div>
            <div style="padding: 0 12px 12px;">
                <For
                    each=move || { filtered.get().into_iter().enumerate().collect::<Vec<_>>() }
                    key=|item| item.1.0.clone()
                    children=move |item| {
                        let list_idx = item.0;
                        let name = item.1.0.clone();
                        let guild = item.1.1.clone();
                        let (r, g, b) = item.1.3;
                        let name_click = name.clone();
                        let on_click = move |_| {
                            selected.set(Some(name_click.clone()));
                            let map = territories.get_untracked();
                            if let Some(ct) = map.get(&name_click) {
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
                        };
                        view! {
                            <div
                                data-sidebar-idx={list_idx.to_string()}
                                style="display: flex; align-items: center; gap: 10px; padding: 7px 10px; border-radius: 4px; cursor: pointer; transition: background 0.15s, box-shadow 0.15s;"
                                style:box-shadow=move || if sidebar_index.get() == list_idx { "inset 2px 0 0 #f5c542" } else { "none" }
                                on:click=on_click
                                on:mouseenter=|e| {
                                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                                        el.style().set_property("background", "#232738").ok();
                                    }
                                }
                                on:mouseleave=|e| {
                                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                                        el.style().set_property("background", "transparent").ok();
                                    }
                                }
                            >
                                <div style={format!("width: 14px; height: 14px; border-radius: 3px; border: 1px solid rgba(255,255,255,0.1); flex-shrink: 0; box-shadow: inset 1px 1px 0 rgba(255,255,255,0.06), inset -1px -1px 0 rgba(0,0,0,0.3); background: {};", rgba_css(r, g, b, 0.8))} />
                                <div style="flex: 1; min-width: 0;">
                                    <div style="font-size: 0.88rem; color: #e2e0d8; font-family: 'Inter', system-ui, sans-serif; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">{name}</div>
                                    <div style="font-size: 0.75rem; color: #9a9590; font-family: 'JetBrains Mono', monospace;">{guild}</div>
                                </div>
                            </div>
                        }
                    }
                />
            </div>
        </div>
    }
}

#[component]
fn LeaderboardPanel() -> impl IntoView {
    let territories: RwSignal<ClientTerritoryMap> = expect_context();
    let Selected(selected) = expect_context();
    let SidebarIndex(sidebar_index) = expect_context();
    let SidebarItems(sidebar_items) = expect_context();
    let CurrentMode(mode) = expect_context();

    let leaderboard = Memo::new(move |_| {
        let map = territories.get();
        let mut guild_counts: HashMap<String, _> = HashMap::new();

        for ct in map.values() {
            let entry = guild_counts
                .entry(ct.territory.guild.name.clone())
                .or_insert_with(|| {
                    (
                        ct.territory.guild.name.clone(),
                        ct.territory.guild.prefix.clone(),
                        0,
                        ct.guild_color,
                    )
                });
            entry.2 += 1;
        }

        let mut sorted: Vec<_> = guild_counts.into_values().collect();
        sorted.sort_by(|a, b| b.2.cmp(&a.2));
        sorted.truncate(20);
        sorted
    });

    // Sync sidebar items for keyboard navigation
    Effect::new(move || {
        let lb = leaderboard.get();
        let map = territories.get_untracked();
        let items: Vec<String> = lb
            .iter()
            .filter_map(|(guild_name, _, _, _)| {
                map.iter()
                    .find(|(_, ct)| ct.territory.guild.name == *guild_name)
                    .map(|(tn, _)| tn.clone())
            })
            .collect();
        let prev = sidebar_items.get_untracked();
        if items != prev {
            sidebar_index.set(0);
        }
        sidebar_items.set(items);
    });

    let is_empty = Memo::new(move |_| territories.get().is_empty());

    view! {
        <div style="border-bottom: 1px solid #282c3e;">
            <div style="padding: 14px 24px 8px; font-family: 'Silkscreen', monospace; font-size: 0.85rem; text-transform: uppercase; letter-spacing: 0.14em; color: #5a5860;">
                <span style="color: #f5c542; margin-right: 6px; font-size: 0.7rem;">{"\u{25C6}"}</span>"Top Guilds"
            </div>
            <Show
                when=move || !is_empty.get()
                fallback=|| view! {
                    <div style="padding: 24px; text-align: center;">
                        <div class="status-pulse" style="font-family: 'JetBrains Mono', monospace; font-size: 0.78rem; color: #3a3f5c; letter-spacing: 0.05em;">"Awaiting territory data..."</div>
                        <div style="margin-top: 12px; display: flex; justify-content: center; gap: 4px;">
                            <div style="width: 4px; height: 4px; border-radius: 50%; background: #f5c542; opacity: 0.3; animation: pulse-dot 1.5s ease-in-out infinite;" />
                            <div style="width: 4px; height: 4px; border-radius: 50%; background: #f5c542; opacity: 0.3; animation: pulse-dot 1.5s ease-in-out 0.3s infinite;" />
                            <div style="width: 4px; height: 4px; border-radius: 50%; background: #f5c542; opacity: 0.3; animation: pulse-dot 1.5s ease-in-out 0.6s infinite;" />
                        </div>
                    </div>
                }
            >
            <ul style="list-style: none; padding: 0 12px 12px;">
                <For
                    each=move || {
                        leaderboard.get().into_iter().enumerate().collect::<Vec<_>>()
                    }
                    key=|item| item.1.0.clone()
                    children=move |item| {
                        let list_idx = item.0;
                        let rank = list_idx + 1;
                        let (name, prefix, count, (r, g, b)) = item.1;
                        let name_for_click = name.clone();
                        let on_click = move |_| {
                            let map = territories.get_untracked();
                            let first = map.iter().find(|(_, ct)| ct.territory.guild.name == name_for_click);
                            if let Some((territory_name, _)) = first {
                                selected.set(Some(territory_name.clone()));
                            }
                        };
                        let rank_class = match rank {
                            1 => "text-gold-gradient",
                            2 => "text-silver-gradient",
                            3 => "text-bronze-gradient",
                            _ => "",
                        };
                        let rank_style = if rank > 3 {
                            "font-family: 'JetBrains Mono', monospace; font-size: 0.75rem; color: #4a4e6a; width: 26px; text-align: right; flex-shrink: 0;"
                        } else {
                            "font-family: 'JetBrains Mono', monospace; font-size: 0.75rem; font-weight: 700; width: 26px; text-align: right; flex-shrink: 0;"
                        };
                        let row_style = if mode.get_untracked() == MapMode::History {
                            "display: flex; align-items: center; gap: 10px; padding: 7px 10px; border-radius: 4px; cursor: pointer; transition: background 0.15s, box-shadow 0.15s;".to_string()
                        } else {
                            let delay_ms = rank * 30;
                            format!(
                                "display: flex; align-items: center; gap: 10px; padding: 7px 10px; border-radius: 4px; cursor: pointer; transition: background 0.15s, box-shadow 0.15s; animation: fade-in-up 0.3s ease-out {}ms both;",
                                delay_ms
                            )
                        };
                        // Top 3 get a subtle left accent
                        let is_podium = rank <= 3;
                        let color_bar_style = if is_podium {
                            format!(
                                "width: 3px; height: 100%; border-radius: 2px; background: {}; flex-shrink: 0; align-self: stretch;",
                                rgba_css(r, g, b, 0.6)
                            )
                        } else {
                            String::new()
                        };
                        view! {
                            <li
                                data-sidebar-idx={list_idx.to_string()}
                                style=row_style
                                style:box-shadow=move || if sidebar_index.get() == list_idx { "inset 2px 0 0 #f5c542" } else { "none" }
                                on:click=on_click
                                on:mouseenter=|e| {
                                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                                        el.style().set_property("background", "#232738").ok();
                                    }
                                }
                                on:mouseleave=|e| {
                                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                                        el.style().set_property("background", "transparent").ok();
                                    }
                                }
                            >
                                {is_podium.then(|| view! { <div style=color_bar_style.clone() /> })}
                                <span class=rank_class style=rank_style>{rank}</span>
                                <div style={format!("width: 16px; height: 16px; border-radius: 3px; border: 1px solid rgba(255,255,255,0.1); flex-shrink: 0; box-shadow: 0 0 4px {}, inset 1px 1px 0 rgba(255,255,255,0.06), inset -1px -1px 0 rgba(0,0,0,0.3); background: {};", rgba_css(r, g, b, 0.15), rgba_css(r, g, b, 0.8))} />
                                <span style="flex: 1; font-size: 0.9rem; color: #e2e0d8; font-family: 'Inter', system-ui, sans-serif; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">{name}</span>
                                <span style="font-size: 0.7rem; color: #9a9590; font-family: 'JetBrains Mono', monospace;">"[" {prefix} "]"</span>
                                <span style="font-size: 0.82rem; color: #f5c542; font-family: 'JetBrains Mono', monospace; min-width: 24px; text-align: right; font-weight: 500; background: rgba(245,197,66,0.06); padding: 1px 6px; border-radius: 3px;">{count}</span>
                            </li>
                        }
                    }
                />
            </ul>
            </Show>
        </div>
    }
}

#[component]
fn DetailPanel() -> impl IntoView {
    let Selected(selected) = expect_context();
    let territories: RwSignal<ClientTerritoryMap> = expect_context();
    let tick: RwSignal<i64> = expect_context();
    let CurrentMode(mode) = expect_context();
    let HistoryTimestamp(history_timestamp) = expect_context();

    let tower_state: crate::tower::TowerState = expect_context();

    // Guild-aware connection counts — only recomputes when territories/selection change, not every tick
    let guild_counts = Memo::new(move |_| {
        let name = selected.get()?;
        let map = territories.get();
        let ct = map.get(&name)?;
        let guild_uuid = ct.territory.guild.uuid.as_str();
        let connections = ct.territory.connections.as_slice();

        let (guild_conn, total_conn, ext) =
            sequoia_shared::tower::count_guild_connections(&name, connections, guild_uuid, |n| {
                let ct2 = map.get(n)?;
                Some((
                    ct2.territory.guild.uuid.as_str(),
                    ct2.territory.connections.as_slice(),
                ))
            });

        Some((guild_conn, total_conn, ext))
    });

    // Sync tower connection counts when territory changes
    Effect::new(move || {
        if let Some((gc, _, ext)) = guild_counts.get() {
            tower_state.connections.set(gc);
            tower_state.externals.set(ext);
        }
    });

    let detail = Memo::new(move |_| {
        let reference_secs = if mode.get() == MapMode::History {
            history_timestamp.get().unwrap_or_else(|| tick.get())
        } else {
            tick.get()
        };
        let name = selected.get()?;
        let map = territories.get();
        let ct = map.get(&name)?;
        let acquired_rfc = ct.territory.acquired.to_rfc3339();
        let treasury = chrono::DateTime::parse_from_rfc3339(&acquired_rfc)
            .ok()
            .map(|dt| {
                let secs = (reference_secs - dt.timestamp()).max(0);
                TreasuryLevel::from_held_seconds(secs)
            })
            .unwrap_or(TreasuryLevel::VeryLow);
        Some((
            name,
            ct.territory.guild.name.clone(),
            ct.territory.guild.prefix.clone(),
            ct.territory.guild.uuid.clone(),
            acquired_rfc,
            ct.territory.location.clone(),
            ct.guild_color,
            treasury,
            ct.territory.resources.clone(),
            ct.territory.connections.len(),
            reference_secs,
        ))
    });

    let on_close = move |_| {
        selected.set(None);
    };

    view! {
        <div class="panel-reveal" style="border-bottom: 1px solid #282c3e; position: relative;">
            <button
                style="position: absolute; top: 12px; right: 12px; background: none; border: none; color: #5a5860; cursor: pointer; padding: 4px 8px; border-radius: 4px; transition: color 0.15s, background 0.15s; z-index: 1; display: flex; align-items: center; justify-content: center;"
                on:click=on_close
                on:mouseenter=|e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("color", "#e05252").ok();
                        el.style().set_property("background", "#232738").ok();
                    }
                }
                on:mouseleave=|e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        el.style().set_property("color", "#5a5860").ok();
                        el.style().set_property("background", "transparent").ok();
                    }
                }
            >
                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" width="16" height="16">
                    <path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z" />
                </svg>
            </button>
            {move || {
                detail
                    .get()
                    .map(|(name, guild_name, guild_prefix, _uuid, acquired, location, (r, g, b), treasury, resources, conn_count, reference_secs)| {
                        let relative_time = format_relative_time(&acquired, reference_secs);
                        let (tr, tg, tb) = treasury.color_rgb();
                        let treasury_label = treasury.label();
                        let buff = treasury.buff_percent();

                        // Cooldown: territory can be queued again after 10 minutes
                        let cooldown = chrono::DateTime::parse_from_rfc3339(&acquired).ok().and_then(|dt| {
                            let age = (reference_secs - dt.timestamp()).max(0);
                            if age < 600 {
                                let remaining = 600 - age;
                                let frac = remaining as f64 / 600.0;
                                Some((format!("{}:{:02}", remaining / 60, remaining % 60), frac))
                            } else {
                                None
                            }
                        });
                        view! {
                            // Guild color accent bar at top
                            <div style={format!(
                                "height: 4px; background: linear-gradient(to right, {}, {} 60%, transparent);",
                                rgba_css(r, g, b, 0.7),
                                rgba_css(r, g, b, 0.3),
                            )} />
                            <div style="padding: 18px 24px 20px;">
                                <div style="display: flex; align-items: center; gap: 10px; margin-bottom: 4px;">
                                    // Guild color swatch
                                    <div style={format!(
                                        "width: 20px; height: 20px; border-radius: 4px; border: 1px solid rgba(255,255,255,0.12); background: {}; flex-shrink: 0; box-shadow: 0 0 6px {}, inset 1px 1px 0 rgba(255,255,255,0.06), inset -1px -1px 0 rgba(0,0,0,0.3);",
                                        rgba_css(r, g, b, 0.8),
                                        rgba_css(r, g, b, 0.3),
                                    )} />
                                    <div style="font-size: 1.15rem; font-weight: 700; color: #e2e0d8; font-family: 'Silkscreen', monospace;">{name}</div>
                                </div>
                                <div style="font-size: 0.95rem; color: #f5c542; font-family: 'Inter', system-ui, sans-serif; margin-bottom: 2px; margin-left: 28px;">{guild_name}</div>
                                <div style="font-size: 0.8rem; color: #9a9590; font-family: 'JetBrains Mono', monospace; margin-bottom: 16px; margin-left: 28px;">
                                    "[" {guild_prefix} "]"
                                </div>

                                // Detail rows with dotted leaders
                                <div style="display: flex; justify-content: space-between; padding: 8px 0; font-size: 0.85rem; border-bottom: 1px solid rgba(40,44,62,0.6);">
                                    <span style="color: #9a9590; font-family: 'Inter', system-ui, sans-serif;">"Acquired"</span>
                                    <span style="color: #e2e0d8; font-family: 'JetBrains Mono', monospace; font-size: 0.78rem;" title=acquired>{relative_time}</span>
                                </div>
                                <div style="display: flex; justify-content: space-between; align-items: center; padding: 8px 0; font-size: 0.85rem; border-bottom: 1px solid rgba(40,44,62,0.6);">
                                    <span style="color: #9a9590; font-family: 'Inter', system-ui, sans-serif;">"Treasury"</span>
                                    <span style="display: flex; align-items: center; gap: 6px;">
                                        <span style={format!("color: {}; font-family: 'JetBrains Mono', monospace; font-size: 0.78rem;", rgba_css(tr, tg, tb, 1.0))}>{treasury_label}</span>
                                        {(buff > 0).then(|| view! {
                                            <span style={format!("font-size: 0.65rem; font-family: 'JetBrains Mono', monospace; color: {}; background: {}; padding: 1px 5px; border-radius: 3px;", rgba_css(tr, tg, tb, 0.9), rgba_css(tr, tg, tb, 0.08))}>{format!("+{}%", buff)}</span>
                                        })}
                                    </span>
                                </div>
                                {cooldown.map(|(remaining_text, frac)| view! {
                                    <div style="padding: 8px 0; border-bottom: 1px solid rgba(40,44,62,0.6);">
                                        <div style="display: flex; justify-content: space-between; font-size: 0.85rem; margin-bottom: 6px;">
                                            <span style="color: #f5c542; font-family: 'Silkscreen', monospace;">"Cooldown"</span>
                                            <span style="color: #f5c542; font-family: 'JetBrains Mono', monospace; font-size: 0.78rem;">{remaining_text}</span>
                                        </div>
                                        <div style="height: 5px; background: rgba(255,255,255,0.06); border-radius: 2px; overflow: hidden;">
                                            <div style={format!(
                                                "height: 100%; width: {:.1}%; background: linear-gradient(to right, #f5c542, #d4a030); border-radius: 2px; transition: width 1s linear; box-shadow: 0 0 6px rgba(245,197,66,0.1);",
                                                frac * 100.0
                                            )} />
                                        </div>
                                    </div>
                                })}
                                <div style="display: flex; justify-content: space-between; padding: 8px 0; font-size: 0.85rem; border-bottom: 1px solid rgba(40,44,62,0.6);">
                                    <span style="color: #9a9590; font-family: 'Inter', system-ui, sans-serif;">"Region"</span>
                                    <span style="color: #e2e0d8; font-family: 'JetBrains Mono', monospace; font-size: 0.78rem;">
                                        {format!(
                                            "({}, {}) \u{2192} ({}, {})",
                                            location.start[0],
                                            location.start[1],
                                            location.end[0],
                                            location.end[1],
                                        )}
                                    </span>
                                </div>
                                <div style="display: flex; justify-content: space-between; padding: 8px 0; font-size: 0.85rem; border-bottom: 1px solid rgba(40,44,62,0.6);">
                                    <span style="color: #9a9590; font-family: 'Inter', system-ui, sans-serif;">"Size"</span>
                                    <span style="color: #e2e0d8; font-family: 'JetBrains Mono', monospace; font-size: 0.78rem;">
                                        {format!(
                                            "{}\u{00d7}{}",
                                            location.width(),
                                            location.height(),
                                        )}
                                    </span>
                                </div>
                                <div style="display: flex; justify-content: space-between; padding: 8px 0; font-size: 0.85rem; border-bottom: 1px solid rgba(40,44,62,0.6);">
                                    <span style="color: #9a9590; font-family: 'Inter', system-ui, sans-serif;">"Connections"</span>
                                    <span style="color: #e2e0d8; font-family: 'JetBrains Mono', monospace; font-size: 0.78rem;">
                                        {move || guild_counts.get().map(|(gc, tc, _)| format!("{}/{}", gc, tc)).unwrap_or_else(|| format!("{}", conn_count))}
                                    </span>
                                </div>
                                {(!resources.is_empty()).then(|| {
                                    let res_items = build_resource_items(&resources);
                                    view! {
                                        <div style="padding: 10px 0 4px;">
                                            <div style="font-family: 'Silkscreen', monospace; font-size: 0.85rem; text-transform: uppercase; letter-spacing: 0.12em; color: #5a5860; margin-bottom: 8px;">
                                                <span style="color: #f5c542; margin-right: 5px; font-size: 0.7rem;">{"\u{25C6}"}</span>"Resources"
                                            </div>
                                            <div style="display: flex; flex-wrap: wrap; gap: 6px;">
                                                {res_items.into_iter().map(|(label, value, icon_name)| view! {
                                                    <div style="display: flex; align-items: center; gap: 5px; background: #1a1d2a; padding: 4px 8px; border-radius: 4px; border: 1px solid #282c3e;">
                                                        <img src={format!("/icons/{icon_name}.svg")} style="width: 14px; height: 14px; flex-shrink: 0; image-rendering: pixelated;" />
                                                        <span style="font-family: 'JetBrains Mono', monospace; font-size: 0.7rem; color: #e2e0d8;">{value}</span>
                                                        <span style="font-family: 'Inter', system-ui, sans-serif; font-size: 0.62rem; color: #5a5860;">{label}</span>
                                                    </div>
                                                }).collect::<Vec<_>>()}
                                            </div>
                                        </div>
                                    }
                                })}
                                <TowerCalculator />
                            </div>
                        }
                    })
            }}
        </div>
    }
}

#[component]
fn StatsBar() -> impl IntoView {
    let territories: RwSignal<ClientTerritoryMap> = expect_context();
    let connection: RwSignal<ConnectionStatus> = expect_context();
    let ShowSettings(show_settings) = expect_context();
    let CurrentMode(mode) = expect_context();
    let PlaybackActive(playback_active) = expect_context();
    let HistoryAvailable(history_available) = expect_context();
    let HistoryTimestamp(history_timestamp) = expect_context();
    let HistoryBoundsSignal(history_bounds) = expect_context();
    let HistoryFetchNonce(history_fetch_nonce) = expect_context();
    let LastLiveSeq(last_live_seq) = expect_context();
    let HistoryBufferedUpdates(history_buffered_updates) = expect_context();
    let HistoryBufferModeActive(history_buffer_mode_active) = expect_context();
    let NeedsLiveResync(needs_live_resync) = expect_context();
    let LiveHandoffResyncCount(live_handoff_resync_count) = expect_context();
    let TerritoryGeometryStore(geo_store) = expect_context();
    let GuildColorStore(guild_color_store) = expect_context();

    let territory_count = Memo::new(move |_| territories.get().len());

    let guild_count = Memo::new(move |_| {
        let map = territories.get();
        let mut guilds: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for ct in map.values() {
            guilds.insert(&ct.territory.guild.name);
        }
        guilds.len()
    });

    let status_dot_style = Memo::new(move |_| match connection.get() {
        ConnectionStatus::Live => {
            "width: 8px; height: 8px; border-radius: 50%; background: #50c878; box-shadow: 0 0 8px rgba(80,200,120,0.5);"
        }
        ConnectionStatus::Connecting => {
            "width: 8px; height: 8px; border-radius: 50%; background: #f5c542; box-shadow: 0 0 8px rgba(245,197,66,0.35); animation: pulse-dot 1.5s ease-in-out infinite;"
        }
        ConnectionStatus::Reconnecting => {
            "width: 8px; height: 8px; border-radius: 50%; background: #f5c542; box-shadow: 0 0 8px rgba(245,197,66,0.35); animation: pulse-dot 1.5s ease-in-out infinite;"
        }
    });

    let status_text = Memo::new(move |_| match connection.get() {
        ConnectionStatus::Live => "Live",
        ConnectionStatus::Connecting => "Connecting...",
        ConnectionStatus::Reconnecting => "Reconnecting...",
    });

    let is_history = move || mode.get() == MapMode::History;

    view! {
        <div style="padding: 10px 12px; border-top: 1px solid #282c3e; display: flex; align-items: center; gap: 6px; font-family: 'JetBrains Mono', monospace; font-size: 0.68rem; color: #6a6870;">
            <button
                style:display=move || if history_available.get() { "flex" } else { "none" }
                style="background: none; border: 1px solid #282c3e; border-radius: 999px; padding: 5px 10px; cursor: pointer; align-items: center; justify-content: center; transition: border-color 0.15s, background 0.15s, color 0.15s; font-size: 0.66rem; min-width: 64px;"
                title=move || if is_history() { "Return to live mode (h)" } else { "View territory history (h)" }
                style:color=move || if is_history() { "#13161f" } else { "#5a5860" }
                style:background=move || if is_history() { "#f5c542" } else { "#1a1d2a" }
                style:border-color=move || if is_history() { "#f5c542" } else { "#282c3e" }
                on:click=move |_| {
                    if is_history() {
                        history::exit_history_mode(history::ExitHistoryModeInput {
                            mode,
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
                    } else {
                        history::enter_history_mode(history::EnterHistoryModeInput {
                            mode,
                            history_timestamp,
                            history_bounds,
                            history_fetch_nonce,
                            history_buffered_updates,
                            history_buffer_mode_active,
                            needs_live_resync,
                            geo_store,
                            guild_color_store,
                            territories,
                        });
                    }
                }
                on:mouseenter=move |e| {
                    if !is_history()
                        && let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                    {
                        el.style().set_property("color", "#9a9590").ok();
                        el.style().set_property("border-color", "#3a3f5c").ok();
                    }
                }
                on:mouseleave=move |e| {
                    if !is_history()
                        && let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                    {
                        el.style().set_property("color", "#5a5860").ok();
                        el.style().set_property("border-color", "#282c3e").ok();
                    }
                }
            >
                {move || if is_history() { "Live" } else { "History" }}
            </button>
            <div style="background: #1a1d2a; border-radius: 999px; padding: 5px 10px; border: 1px solid #282c3e; display: flex; align-items: center; gap: 4px;">
                <span style="color: #9a9590;">{move || territory_count.get()}</span>
                <span>" terr."</span>
            </div>
            <div style="background: #1a1d2a; border-radius: 999px; padding: 5px 10px; border: 1px solid #282c3e; display: flex; align-items: center; gap: 4px;">
                <span style="color: #9a9590;">{move || guild_count.get()}</span>
                <span>" guilds"</span>
            </div>
            <div
                title=move || status_text.get()
                style="margin-left: auto; width: 26px; height: 26px; border: 1px solid #282c3e; border-radius: 999px; background: #1a1d2a; display: flex; align-items: center; justify-content: center; flex-shrink: 0;"
            >
                <span style=move || status_dot_style.get()></span>
            </div>
            <button
                style="background: none; border: 1px solid #282c3e; border-radius: 999px; padding: 5px 7px; cursor: pointer; display: flex; align-items: center; justify-content: center; transition: border-color 0.15s, background 0.15s, color 0.15s;"
                style:color=move || if show_settings.get() { "#f5c542" } else { "#5a5860" }
                style:background=move || if show_settings.get() { "rgba(245,197,66,0.06)" } else { "#1a1d2a" }
                style:border-color=move || if show_settings.get() { "rgba(245,197,66,0.25)" } else { "#282c3e" }
                on:click=move |_| show_settings.update(|v| *v = !*v)
                on:mouseenter=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                        && !show_settings.get_untracked()
                    {
                        el.style().set_property("color", "#9a9590").ok();
                        el.style().set_property("border-color", "#3a3f5c").ok();
                    }
                }
                on:mouseleave=move |e| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                        && !show_settings.get_untracked()
                    {
                        el.style().set_property("color", "#5a5860").ok();
                        el.style().set_property("border-color", "#282c3e").ok();
                    }
                }
            >
                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" width="14" height="14">
                    <path fill-rule="evenodd" d="M7.84 1.804A1 1 0 018.82 1h2.36a1 1 0 01.98.804l.331 1.652a6.993 6.993 0 011.929 1.115l1.598-.54a1 1 0 011.186.447l1.18 2.044a1 1 0 01-.205 1.251l-1.267 1.113a7.047 7.047 0 010 2.228l1.267 1.113a1 1 0 01.206 1.25l-1.18 2.045a1 1 0 01-1.187.447l-1.598-.54a6.993 6.993 0 01-1.929 1.115l-.33 1.652a1 1 0 01-.98.804H8.82a1 1 0 01-.98-.804l-.331-1.652a6.993 6.993 0 01-1.929-1.115l-1.598.54a1 1 0 01-1.186-.447l-1.18-2.044a1 1 0 01.205-1.251l1.267-1.114a7.05 7.05 0 010-2.227L1.821 7.773a1 1 0 01-.206-1.25l1.18-2.045a1 1 0 011.187-.447l1.598.54A6.993 6.993 0 017.51 3.456l.33-1.652zM10 13a3 3 0 100-6 3 3 0 000 6z" clip-rule="evenodd" />
                </svg>
            </button>
        </div>
    }
}
